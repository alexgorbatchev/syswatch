//! The process tree's scrolling list, shared by every process view.
//!
//! Drawn in Dense's style everywhere: a `▶` selection marker, labels toned by
//! whole-machine CPU, right-aligned readouts, memory under 1 GiB in gray and
//! braille CPU history. Each view supplies only its columns; the name, process
//! count and PID columns, selection, scrolling and narrow-terminal fitting
//! live here so the views cannot drift apart.

use std::collections::HashSet;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::app::App;
use crate::process_tree::{system_only, GroupMode, RowKey, TreeRow};
use crate::ui::dense::paint::{self, Ramps};
use crate::ui::dense::{truncate, Series};
use crate::ui::scroll::ScrollWindow;
use crate::ui::theme::{self, Theme};
use crate::ui::widgets::human_bytes;

const GIB: u64 = 1024 * 1024 * 1024;
/// The name starts after the selection marker and two spaces.
const NAME_X: u16 = 3;
/// Readouts are dropped, rightmost first, before the name gets narrower.
const NAME_MIN: u16 = 18;
/// Below this a history column says nothing; it is dropped instead.
const MIN_SHARE: u16 = 6;

/// What one cell shows.
pub(crate) enum Cell {
    Blank,
    /// Right-aligned text.
    Text(String, Color),
    /// Left-aligned text.
    Left(String, Color),
    /// Braille history on a fixed ceiling, oldest sample first.
    Spark(Vec<u64>, u64),
    /// A bounded meter, 0.0..=1.0.
    Meter(f32),
}

/// The colours a cell resolves against, for its row's selection state.
pub(crate) struct Tone<'t> {
    pub theme: &'t Theme,
    pub ramps: &'t Ramps,
    /// Secondary text: lighter on the selected row for contrast.
    pub muted: Color,
}

impl Tone<'_> {
    /// Memory under 1 GiB recedes; larger amounts keep the memory colour.
    pub fn bytes(&self, value: Option<u64>) -> Cell {
        match value {
            Some(b) => Cell::Text(
                human_bytes(b),
                if b < GIB {
                    self.muted
                } else {
                    self.ramps.secondary.mid()
                },
            ),
            None => Cell::Text("—".into(), self.muted),
        }
    }

    /// A rate in `color` while anything moves, muted at zero.
    pub fn rate(&self, bytes_per_sec: f64, color: Color) -> Cell {
        Cell::Text(
            format!("{}/s", human_bytes(bytes_per_sec.max(0.0) as u64)),
            if bytes_per_sec > 0.0 {
                color
            } else {
                self.muted
            },
        )
    }

    /// Whole-machine CPU: warning at 30%, critical at 70%.
    pub fn cpu(&self, pct: Option<f32>) -> Cell {
        match pct {
            Some(pct) => Cell::Text(format!("{pct:.0}%"), self.usage(pct)),
            None => Cell::Text("n/a".into(), self.muted),
        }
    }

    pub fn usage(&self, pct: f32) -> Color {
        if pct >= 70.0 {
            self.theme.status_error
        } else if pct >= 30.0 {
            self.theme.status_warn
        } else {
            self.theme.status_good
        }
    }
}

enum Width {
    Fixed(u16),
    /// `1/divisor` of the table, at most `max`.
    Share {
        divisor: u16,
        max: u16,
    },
}

type Render<'a, 's> = Box<dyn Fn(&TreeRow<'s>, &Tone) -> Cell + 'a>;

/// One readout column after the name, count and PID.
pub(crate) struct Column<'a, 's> {
    header: String,
    width: Width,
    left: bool,
    sorted: bool,
    cell: Render<'a, 's>,
}

impl<'a, 's> Column<'a, 's> {
    /// A right-aligned column of a fixed width.
    pub fn new(
        header: impl Into<String>,
        width: u16,
        cell: impl Fn(&TreeRow<'s>, &Tone) -> Cell + 'a,
    ) -> Self {
        Self {
            header: header.into(),
            width: Width::Fixed(width),
            left: false,
            sorted: false,
            cell: Box::new(cell),
        }
    }

    /// A left-aligned column taking a share of the table's width.
    pub fn share(
        header: impl Into<String>,
        divisor: u16,
        max: u16,
        cell: impl Fn(&TreeRow<'s>, &Tone) -> Cell + 'a,
    ) -> Self {
        Self {
            width: Width::Share { divisor, max },
            left: true,
            ..Self::new(header, 0, cell)
        }
    }

    pub fn left(mut self) -> Self {
        self.left = true;
        self
    }

    /// Accent the header of the column the rows are ranked by.
    pub fn sorted(mut self, sorted: bool) -> Self {
        self.sorted = sorted;
        self
    }

    pub fn cpu() -> Self {
        Self::new("CPU", 5, |row, tone| tone.cpu(row.totals.cpu_pct))
    }

    /// Resident memory, summed for groups.
    pub fn memory() -> Self {
        Self::new("MEM", 10, |row, tone| tone.bytes(Some(row.totals.mem_rss)))
    }

    /// Each top-level group's recorded share of the machine. Deeper rows have
    /// no history of their own.
    pub fn recent(app: &'a App, mode: GroupMode) -> Self {
        Self::share("RECENT", 4, 28, move |row, tone| {
            if row.depth > 0 || row.totals.cpu_pct.is_none() {
                return Cell::Blank;
            }
            let mut values: Vec<u64> = (app.scrub_offset..app.scrub_offset + 28)
                .map_while(|offset| app.history.cpu_groups.nth_back(offset))
                .map(|point| paint::perceptual(point.get(mode, &row.group) / 100.0, 64))
                .collect();
            values.reverse();
            if values.len() > 1 {
                Cell::Spark(values, 64)
            } else {
                Cell::Left("·".into(), tone.muted)
            }
        })
    }
}

/// A tree of process rows with its readout columns.
pub(crate) struct TreeTable<'a, 's> {
    rows: &'a [TreeRow<'s>],
    title: &'a str,
    expanded: Option<&'a HashSet<RowKey>>,
    columns: Vec<Column<'a, 's>>,
    selection: Option<(usize, &'a ScrollWindow)>,
    empty: &'a str,
}

impl<'a, 's> TreeTable<'a, 's> {
    pub fn new(rows: &'a [TreeRow<'s>], title: &'a str) -> Self {
        Self {
            rows,
            title,
            expanded: None,
            columns: Vec::new(),
            selection: None,
            empty: "No process samples available.",
        }
    }

    /// Open folder icons for expanded rows.
    pub fn expanded(mut self, expanded: &'a HashSet<RowKey>) -> Self {
        self.expanded = Some(expanded);
        self
    }

    pub fn columns(mut self, columns: impl IntoIterator<Item = Column<'a, 's>>) -> Self {
        self.columns.extend(columns);
        self
    }

    /// Mark `selected` and keep it inside `scroll`'s window. Without one the
    /// table is a read-only summary starting at its first row.
    pub fn selection(mut self, selected: usize, scroll: &'a ScrollWindow) -> Self {
        self.selection = Some((selected, scroll));
        self
    }

    pub fn empty(mut self, message: &'a str) -> Self {
        self.empty = message;
        self
    }
}

/// The built-in columns, then the view's own by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Count,
    Pid,
    Column(usize),
}

/// A column's resolved position.
struct Placed {
    slot: Slot,
    x: u16,
    width: u16,
}

impl Widget for TreeTable<'_, '_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = theme::active();
        let ramps = Ramps::from_theme(&theme);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(' ');
                    cell.set_style(Style::default().bg(theme.bg));
                }
            }
        }
        if area.height == 0 || area.width <= NAME_X + 1 {
            return;
        }

        let pid_width = self
            .rows
            .iter()
            .filter_map(|row| row.process.map(|p| p.pid.to_string().len()))
            .max()
            .unwrap_or(0)
            .max(3) as u16;
        let (name_width, placed) = self.layout(area, pid_width);

        let name_x = area.x + NAME_X;
        let header = Style::default().fg(theme.border);
        paint::put(
            buf,
            name_x,
            area.y,
            &truncate(self.title, name_width as usize),
            header,
        );
        for place in &placed {
            let (title, left, sorted) = match place.slot {
                Slot::Count => ("PROCS", false, false),
                Slot::Pid => ("PID", false, false),
                Slot::Column(i) => {
                    let column = &self.columns[i];
                    (column.header.as_str(), column.left, column.sorted)
                }
            };
            let style = if sorted {
                Style::default()
                    .fg(theme.brand)
                    .add_modifier(Modifier::BOLD)
            } else {
                header
            };
            draw(
                buf,
                place,
                area.y,
                &if left {
                    Cell::Left(title.into(), Color::Reset)
                } else {
                    Cell::Text(title.into(), Color::Reset)
                },
                style,
                &ramps,
                &theme,
            );
        }

        let visible = area.height.saturating_sub(1) as usize;
        if self.rows.is_empty() {
            if visible > 0 {
                paint::put(
                    buf,
                    name_x,
                    area.y + 1,
                    &truncate(self.empty, name_width as usize),
                    Style::default().fg(theme.text_muted),
                );
            }
            return;
        }
        let first = self.selection.map_or(0, |(selected, scroll)| {
            scroll.follow(selected, self.rows.len(), visible)
        });
        let prefixes = prefixes(self.rows);
        for (offset, (row, prefix)) in self
            .rows
            .iter()
            .zip(&prefixes)
            .skip(first)
            .take(visible)
            .enumerate()
        {
            let y = area.y + 1 + offset as u16;
            let selected = self
                .selection
                .is_some_and(|(selected, _)| selected == first + offset);
            let tone = Tone {
                theme: &theme,
                ramps: &ramps,
                muted: if selected {
                    theme.text_secondary
                } else {
                    theme.text_muted
                },
            };
            if selected {
                for x in area.left()..area.right() {
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_bg(theme.selection_bg);
                    }
                }
                paint::put(buf, area.x + 1, y, "▶", Style::default().fg(theme.brand));
            }
            let expanded = self
                .expanded
                .is_some_and(|expanded| expanded.contains(&row.key));
            buf.set_line(name_x, y, &name(row, prefix, expanded, &tone), name_width);
            for place in &placed {
                let cell = match place.slot {
                    Slot::Column(i) => (self.columns[i].cell)(row, &tone),
                    Slot::Count => match row.count {
                        1 => Cell::Blank,
                        count => Cell::Text(count.to_string(), tone.muted),
                    },
                    Slot::Pid => row
                        .process
                        .map_or(Cell::Blank, |p| Cell::Text(p.pid.to_string(), tone.muted)),
                };
                draw(buf, place, y, &cell, Style::default(), &ramps, &theme);
            }
        }
    }
}

impl TreeTable<'_, '_> {
    /// Fit the count, PID and fixed readouts from the left while the name
    /// keeps its minimum, then size shared columns from what remains. The name
    /// takes the rest.
    fn layout(&self, area: Rect, pid_width: u16) -> (u16, Vec<Placed>) {
        let width = area.width.saturating_sub(NAME_X + 1);
        let fixed = [(Slot::Count, 5), (Slot::Pid, pid_width)]
            .into_iter()
            .chain(
                self.columns
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| match c.width {
                        Width::Fixed(w) => Some((Slot::Column(i), w)),
                        Width::Share { .. } => None,
                    }),
            );
        let mut used = 0;
        let mut widths = Vec::new();
        for (slot, w) in fixed {
            if used + w + 1 + NAME_MIN <= width {
                used += w + 1;
                widths.push((slot, w));
            }
        }
        for (i, column) in self.columns.iter().enumerate() {
            if let Width::Share { divisor, max } = column.width {
                let w = (width / divisor)
                    .min(max)
                    .min(width.saturating_sub(used + 1 + NAME_MIN));
                if w >= MIN_SHARE {
                    used += w + 1;
                    widths.push((Slot::Column(i), w));
                }
            }
        }
        // Columns keep their declared order: count, PID, then the view's own.
        widths.sort_by_key(|(slot, _)| *slot);
        let name_width = width - used;
        let mut x = area.x + NAME_X + name_width;
        let placed = widths
            .into_iter()
            .map(|(slot, width)| {
                let place = Placed {
                    slot,
                    x: x + 1,
                    width,
                };
                x += width + 1;
                place
            })
            .collect();
        (name_width, placed)
    }
}

fn draw(
    buf: &mut Buffer,
    place: &Placed,
    y: u16,
    cell: &Cell,
    style: Style,
    ramps: &Ramps,
    theme: &Theme,
) {
    let fg = |color: Color| {
        if color == Color::Reset {
            style
        } else {
            style.fg(color)
        }
    };
    match cell {
        Cell::Blank => {}
        Cell::Text(text, color) => {
            paint::put_right(
                buf,
                place.x + place.width - 1,
                y,
                &truncate(text, place.width as usize),
                fg(*color),
            );
        }
        Cell::Left(text, color) => {
            paint::put(
                buf,
                place.x,
                y,
                &truncate(text, place.width as usize),
                fg(*color),
            );
        }
        Cell::Spark(values, max) => {
            let series = Series::fixed(values.clone(), place.width, *max);
            paint::spark(
                buf,
                place.x,
                y,
                place.width,
                &series.plot(),
                series.max,
                &ramps.primary,
            );
        }
        Cell::Meter(frac) => {
            paint::meter(
                buf,
                place.x,
                y,
                place.width,
                *frac,
                &ramps.load,
                theme.border,
            );
        }
    }
}

/// Branch prefix, icon and label toned by the row's CPU, with container IDs,
/// `(deleted)` and `[system]` receding.
fn name<'l>(row: &'l TreeRow<'_>, prefix: &str, expanded: bool, tone: &Tone) -> Line<'l> {
    let label = match row.totals.cpu_pct {
        Some(pct) if pct >= 30.0 => tone.theme.status_error,
        Some(pct) if pct >= 10.0 => tone.theme.text_primary,
        _ => tone.muted,
    };
    let quiet = Style::default().fg(tone.muted);
    let mut line = Line::from(Span::raw(format!("{prefix}{} ", icon(row, expanded))))
        .style(Style::default().fg(label));
    if let (RowKey::Container(_), Some((kind, id))) = (&row.key, row.label.split_once(' ')) {
        line.spans.push(Span::raw(format!("{kind} ")));
        line.spans.push(Span::styled(id, quiet));
    } else if let Some((before, after)) = row.label.split_once("(deleted)") {
        line.spans.push(Span::raw(before));
        line.spans.push(Span::styled("(deleted)", quiet));
        line.spans.push(Span::raw(after));
    } else {
        line.spans.push(Span::raw(row.label.as_str()));
    }
    if system_only(&row.members) {
        line.spans
            .push(Span::styled(" [system]", quiet.add_modifier(Modifier::DIM)));
    }
    line
}

fn icon(row: &TreeRow<'_>, expanded: bool) -> char {
    if row.icon == '\u{f07b}' && expanded {
        '\u{f07c}'
    } else {
        row.icon
    }
}

fn prefixes(rows: &[TreeRow<'_>]) -> Vec<String> {
    from_depths(&rows.iter().map(|row| row.depth).collect::<Vec<_>>())
        .into_iter()
        .zip(rows)
        .map(|(prefix, row)| {
            if row.process.is_some() {
                prefix.replace('─', "")
            } else {
                prefix
            }
        })
        .collect()
}

fn from_depths(depths: &[usize]) -> Vec<String> {
    // The next row at the same or a shallower depth determines whether a
    // branch has another sibling. Descendants do not affect that decision.
    let mut pending = Vec::new();
    let mut last = vec![true; depths.len()];
    for (index, &depth) in depths.iter().enumerate().rev() {
        while pending.last().is_some_and(|&next| next > depth) {
            pending.pop();
        }
        last[index] = pending.last() != Some(&depth);
        pending.push(depth);
    }

    let mut ancestors = Vec::new();
    depths
        .iter()
        .enumerate()
        .map(|(index, &depth)| {
            ancestors.truncate(depth);
            let mut prefix = String::new();
            if depth > 0 {
                for &is_last in ancestors.iter().skip(1) {
                    prefix.push_str(if is_last { "   " } else { "│  " });
                }
                prefix.push_str(if last[index] { "╰─ " } else { "├─ " });
            }
            ancestors.push(last[index]);
            prefix
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::{ProcTick, ProcessScope};
    use crate::process_tree::{GroupKey, Totals};
    use crate::ui::palette as p;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn group<'a>(label: &str, members: Vec<&'a ProcTick>, cpu: f32, rss: u64) -> TreeRow<'a> {
        TreeRow {
            key: RowKey::Group(GroupKey::Application {
                identity: format!("name:{label}"),
            }),
            label: label.into(),
            icon: 'x',
            depth: 0,
            totals: Totals {
                cpu_pct: Some(cpu),
                mem_rss: rss,
                ..Totals::default()
            },
            count: members.len(),
            process: None,
            owner: None,
            members,
            group: GroupKey::Application {
                identity: format!("name:{label}"),
            },
        }
    }

    fn leaf(p: &ProcTick, depth: usize) -> TreeRow<'_> {
        TreeRow {
            key: RowKey::Process(p.pid, None),
            depth,
            count: 1,
            process: Some(p),
            ..group(&p.name, vec![p], 0.0, p.mem_rss)
        }
    }

    fn render(table: TreeTable<'_, '_>, width: u16, height: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        table.render(buf.area, &mut buf);
        buf
    }

    fn line(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    fn find(buf: &Buffer, y: u16, text: &str) -> u16 {
        let row = line(buf, y);
        let byte = row
            .find(text)
            .unwrap_or_else(|| panic!("{text:?} in {row:?}"));
        row[..byte].chars().count() as u16
    }

    #[test]
    fn narrow_tables_drop_readouts_before_squeezing_names() {
        let p = ProcTick {
            pid: 1234567,
            name: "worker".into(),
            ..Default::default()
        };
        let rows = [leaf(&p, 0)];
        let table = || {
            TreeTable::new(&rows, "APPLICATION / PROJECT").columns([
                Column::cpu(),
                Column::memory(),
                Column::share("RECENT", 4, 28, |_, _| Cell::Blank),
            ])
        };
        let wide = render(table(), 120, 3);
        let header = line(&wide, 0);
        for title in ["PROCS", "PID", "CPU", "MEM", "RECENT"] {
            assert!(header.contains(title), "{header}");
        }
        // Seven-digit PIDs widen the PID column instead of being clipped.
        assert!(line(&wide, 1).contains("1234567"));
        let pid_end = find(&wide, 0, "PID") + 2;
        assert_eq!(find(&wide, 1, "1234567") + 6, pid_end);

        // 40 columns leave room for the name, count and PID only.
        let narrow = render(table(), 40, 3);
        let header = line(&narrow, 0);
        assert!(
            header.contains("PID") && !header.contains("CPU"),
            "{header}"
        );
        assert!(!header.contains("RECENT"), "{header}");
        assert!(find(&narrow, 1, "worker") >= NAME_X);
    }

    #[test]
    fn selection_marks_and_scrolls_while_summaries_start_at_the_top() {
        let procs: Vec<_> = (0..6)
            .map(|pid| ProcTick {
                pid,
                name: format!("p{pid}"),
                ..Default::default()
            })
            .collect();
        let rows: Vec<_> = procs.iter().map(|p| leaf(p, 0)).collect();
        let scroll = ScrollWindow::default();
        let buf = render(TreeTable::new(&rows, "NAME").selection(5, &scroll), 40, 4);
        // Three visible rows follow the selection to the end of the list.
        assert!(line(&buf, 1).contains("p3") && line(&buf, 3).contains("p5"));
        assert_eq!(buf[(1, 3)].symbol(), "▶");
        assert_eq!(buf[(10, 3)].bg, p::selection_bg());
        assert_ne!(buf[(10, 2)].bg, p::selection_bg());

        let summary = render(TreeTable::new(&rows, "NAME"), 40, 4);
        assert!(line(&summary, 1).contains("p0"));
        assert!(!(0..4).any(|y| line(&summary, y).contains('▶')));

        let empty = render(TreeTable::new(&[], "NAME").empty("Nothing here."), 40, 3);
        assert!(line(&empty, 1).contains("Nothing here."));
    }

    #[test]
    fn names_tone_by_cpu_and_quieten_annotations() {
        let system = ProcTick {
            pid: 1,
            name: "sshd".into(),
            scope: ProcessScope::System,
            ..Default::default()
        };
        let tool = ProcTick {
            pid: 2,
            name: "tool".into(),
            ..Default::default()
        };
        let mut container = group("container abcdef123456", vec![&tool], 12.0, 0);
        container.key = RowKey::Container("abcdef123456".into());
        let rows = [
            group("hot", vec![&tool], 45.0, 0),
            group("worker (deleted)", vec![&tool], 12.0, 0),
            group("sshd", vec![&system], 0.0, 0),
            container,
        ];
        let buf = render(TreeTable::new(&rows, "NAME"), 60, 5);
        let at = |y, text| buf[(find(&buf, y, text), y)].fg;
        assert_eq!(at(1, "hot"), p::status_error());
        assert_eq!(at(2, "worker"), p::text_primary());
        assert_eq!(at(2, "(deleted)"), p::text_muted());
        assert_eq!(at(3, "[system]"), p::text_muted());
        assert!(buf[(find(&buf, 3, "[system]"), 3)]
            .modifier
            .contains(Modifier::DIM));
        assert_eq!(at(4, "container"), p::text_primary());
        assert_eq!(at(4, "abcdef123456"), p::text_muted());
    }

    #[test]
    fn readouts_use_dense_tones_and_the_sorted_header_is_accented() {
        let p = ProcTick {
            pid: 9,
            name: "big".into(),
            ..Default::default()
        };
        let rows = [
            group("big", vec![&p], 75.0, 2 * GIB),
            group("small", vec![&p], 35.0, GIB / 2),
            group("idle", vec![&p], 1.0, 0),
        ];
        let buf = render(
            TreeTable::new(&rows, "NAME").columns([
                Column::cpu().sorted(true),
                Column::memory(),
                Column::new("RATE", 10, |row, tone| {
                    tone.rate(row.totals.cpu_pct.unwrap_or(0.0) as f64 - 1.0, Color::Cyan)
                }),
                Column::new("CUT", 3, |_, _| {
                    Cell::Left("truncated".into(), Color::Reset)
                }),
            ]),
            100,
            4,
        );
        let at = |y, text| buf[(find(&buf, y, text), y)].fg;
        assert_eq!(at(0, "CPU"), p::brand());
        assert!(buf[(find(&buf, 0, "CPU"), 0)]
            .modifier
            .contains(Modifier::BOLD));
        assert_eq!(at(0, "MEM"), p::border());
        assert_eq!(at(1, "75%"), p::status_error());
        assert_eq!(at(2, "35%"), p::status_warn());
        assert_eq!(at(3, "1%"), p::status_good());
        assert_eq!(
            at(1, "2.0 GB"),
            Ramps::from_theme(&theme::active()).secondary.mid()
        );
        assert_eq!(at(2, "512.0 MB"), p::text_muted());
        assert_eq!(at(1, "74 B/s"), Color::Cyan);
        assert_eq!(at(3, "0 B/s"), p::text_muted());
        assert!(line(&buf, 1).ends_with(" tru "), "{}", line(&buf, 1));

        let tone = Tone {
            theme: &theme::active(),
            ramps: &Ramps::from_theme(&theme::active()),
            muted: Color::Gray,
        };
        assert!(matches!(tone.bytes(None), Cell::Text(t, Color::Gray) if t == "—"));
        assert!(matches!(tone.cpu(None), Cell::Text(t, Color::Gray) if t == "n/a"));
    }

    #[test]
    fn history_and_meter_cells_draw_braille_and_bars() {
        let p = ProcTick::default();
        let rows = [group("busy", vec![&p], 50.0, 0), leaf(&p, 1)];
        let buf = render(
            TreeTable::new(&rows, "NAME").columns([
                Column::share("HIST", 4, 10, |row, tone| match row.depth {
                    0 => Cell::Spark(vec![10, 40, 64], 64),
                    _ => Cell::Left("·".into(), tone.muted),
                }),
                Column::share("BAR", 4, 10, |_, _| Cell::Meter(0.5)),
            ]),
            100,
            3,
        );
        let hist = find(&buf, 0, "HIST");
        let bar = find(&buf, 0, "BAR");
        let cells = |y: u16, from: u16| -> String {
            (from..from + 10).map(|x| buf[(x, y)].symbol()).collect()
        };
        assert!(cells(1, hist)
            .chars()
            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)));
        assert!(cells(2, hist).starts_with('·'));
        assert_eq!(cells(1, bar), "■■■■■·····");
    }

    #[test]
    fn recent_history_is_for_top_level_groups_only() {
        let app = App::new(
            crate::app::TabId::Cpu,
            crate::config::SyswatchConfig::default(),
        );
        let p = ProcTick::default();
        let column = Column::recent(&app, GroupMode::Application);
        let theme = theme::active();
        let ramps = Ramps::from_theme(&theme);
        let tone = Tone {
            theme: &theme,
            ramps: &ramps,
            muted: Color::Gray,
        };
        assert!(matches!((column.cell)(&leaf(&p, 1), &tone), Cell::Blank));
        let mut unknown = group("g", vec![&p], 0.0, 0);
        unknown.totals.cpu_pct = None;
        assert!(matches!((column.cell)(&unknown, &tone), Cell::Blank));
        // No recorded history yet: a single dot rather than an empty graph.
        assert!(matches!(
            (column.cell)(&group("g", vec![&p], 1.0, 0), &tone),
            Cell::Left(dot, Color::Gray) if dot == "·"
        ));
    }

    #[test]
    fn connects_siblings_and_nested_branches_without_connecting_roots() {
        assert_eq!(
            from_depths(&[0, 1, 2, 3, 2, 1, 2, 0, 1, 0]),
            [
                "",
                "├─ ",
                "│  ├─ ",
                "│  │  ╰─ ",
                "│  ╰─ ",
                "╰─ ",
                "   ╰─ ",
                "",
                "╰─ ",
                ""
            ]
        );
    }

    #[test]
    fn empty_and_collapsed_trees_have_no_connectors() {
        assert!(from_depths(&[]).is_empty());
        assert_eq!(from_depths(&[0, 0]), ["", ""]);
    }
}
