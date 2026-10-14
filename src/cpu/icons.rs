//! Presentation only: sharing an icon never merges process identities.
//! Glyph names/codepoints: https://github.com/ryanoasis/nerd-fonts/blob/master/glyphnames.json

pub(super) fn for_name(name: &str) -> char {
    if let Some((_, icon)) = known_application(name) {
        return icon;
    }
    match name {
        "safari" => '\u{f267}', // fa-safari
        "msedge" | "microsoft edge" | "microsoft-edge" | "microsoft-edge-stable" => '\u{f282}',
        "opera" => '\u{f26a}',
        "thunderbird" => '\u{f370}', // linux-thunderbird
        "telegram" | "telegram-desktop" => '\u{f2c6}',
        "steam" | "steamwebhelper" => '\u{f1b6}',
        "vlc" => '\u{f057c}', // md-vlc
        "obsidian" => '\u{e6bb}',
        "nvim" | "neovim" => '\u{e6ae}',
        "vim" | "gvim" => '\u{e7c5}',
        "emacs" => '\u{e632}',
        "subl" | "sublime_text" | "sublime text" => '\u{e7aa}',
        "idea" | "idea64" | "intellij idea" | "intellij idea ce" => '\u{e7b5}',
        "pycharm" | "pycharm64" | "pycharm ce" => '\u{e877}',
        "webstorm" | "webstorm64" => '\u{e8e4}',
        "goland" | "goland64" => '\u{e7ef}',
        "phpstorm" | "phpstorm64" => '\u{e862}',
        "android studio" | "studio" | "studio64" => '\u{e71a}',
        "node" | "nodejs" | "npm" | "npx" => '\u{e719}',
        "bun" => '\u{e76f}',
        "deno" => '\u{e7c0}',
        "python" | "python2" | "python3" => '\u{e73c}',
        "rustc" | "cargo" | "rust-analyzer" => '\u{e7a8}',
        "go" | "gopls" => '\u{e724}',
        "java" | "javac" => '\u{e738}',
        "ruby" | "irb" => '\u{e739}',
        "php" | "php-fpm" => '\u{e73d}',
        "git" => '\u{e702}',
        "docker" | "dockerd" | "docker desktop" | "com.docker.backend" => '\u{e7b0}',
        "kubectl" | "kubelet" | "kube-apiserver" => '\u{e81d}',
        "postgres" | "postmaster" => '\u{e76e}',
        "mysqld" | "mariadbd" => '\u{e704}',
        "redis-server" | "redis-cli" => '\u{e76d}',
        "nginx" => '\u{e776}',
        "sh"
        | "bash"
        | "zsh"
        | "fish"
        | "dash"
        | "terminal"
        | "iterm2"
        | "alacritty"
        | "kitty"
        | "wezterm"
        | "ghostty"
        | "gnome-terminal-server" => '\u{f120}',
        "pwsh" | "powershell" => '\u{e86c}',
        "tmux" => '\u{ebc8}',
        "codex" | "claude" => '\u{ee0d}', // fa-robot: category, not a brand logo
        "systemd" | "init" | "launchd" | "services" | "svchost" | "dbus-daemon" | "dbus-broker"
        | "systemd-journald" | "systemd-udevd" => '\u{f085}', // fa-cogs
        "networkmanager" | "systemd-networkd" | "systemd-resolved" | "wpa_supplicant"
        | "avahi-daemon" | "mdnsresponder" | "configd" => '\u{f06f3}', // md-network
        "pipewire" | "pipewire-pulse" | "wireplumber" | "pulseaudio" | "coreaudiod" | "audiodg" => {
            '\u{f028}'
        } // fa-volume_high
        "sshd" | "polkitd" | "securityd" | "lsass" => '\u{f511}', // oct-shield_lock
        "cron" | "crond" | "atd" | "systemd-timesyncd" | "chronyd" | "ntpd" => '\u{f017}',
        "bluetoothd" => '\u{f293}',
        "mds" | "mds_stores" | "mdworker" | "mdworker_shared" | "searchindexer" => '\u{f002}',
        "windowserver" | "xorg" | "xwayland" | "gnome-shell" | "kwin_wayland" | "kwin_x11"
        | "dwm" => '\u{f0379}', // md-monitor
        "kernel_task" | "kthreadd" => '\u{f2db}', // fa-microchip
        "powerd" | "upowerd" => '\u{f0e7}',
        _ if python_version(name) => '\u{e73c}',
        _ => '\u{eb7f}', // cod-window
    }
}

pub(super) fn known_application(name: &str) -> Option<(&'static str, char)> {
    match name {
        "google chrome" | "google-chrome" | "google-chrome-stable" | "chrome" => {
            Some(("Google Chrome", '\u{f268}'))
        }
        "chromium" | "chromium-browser" => Some(("Chromium", '\u{f268}')),
        "firefox" | "firefox-bin" => Some(("Firefox", '\u{f269}')),
        "visual studio code" | "code" => Some(("Visual Studio Code", '\u{e8da}')),
        "slack" => Some(("Slack", '\u{f198}')),
        "discord" => Some(("Discord", '\u{f1ff}')),
        "spotify" => Some(("Spotify", '\u{f1bc}')),
        _ => None,
    }
}

fn python_version(name: &str) -> bool {
    name.strip_prefix("python")
        .and_then(|version| version.split_once('.'))
        .is_some_and(|(major, minor)| {
            matches!(major, "2" | "3")
                && !minor.is_empty()
                && minor.bytes().all(|b| b.is_ascii_digit())
        })
}
