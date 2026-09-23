use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
pub struct Spinner {
    pub frames: &'static [&'static str],
    pub fps: u8,
}

impl Spinner {
    pub const DOT: Spinner = Spinner {
        frames: &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"],
        fps: 10,
    };
    pub const LINE: Spinner = Spinner {
        frames: &["|", "/", "-", "\\"],
        fps: 10,
    };
    pub const JUMP: Spinner = Spinner {
        frames: &["⢄", "⢂", "⢁", "⡁", "⡈", "⡐", "⡠"],
        fps: 10,
    };
    pub const PULSE: Spinner = Spinner {
        frames: &["█", "▓", "▒", "░"],
        fps: 10,
    };
    pub const POINTS: Spinner = Spinner {
        frames: &["∙∙∙", "●∙∙", "∙●∙", "∙∙●", "∙∙∙"],
        fps: 7,
    };
    pub const METER: Spinner = Spinner {
        frames: &["▱▱▱", "▰▱▱", "▰▰▱", "▰▰▰", "▰▰▱", "▰▱▱"],
        fps: 7,
    };
    pub const HAMBURGER: Spinner = Spinner {
        frames: &["☱", "☲", "☴", "☲"],
        fps: 10,
    };
    pub const ELLIPSIS: Spinner = Spinner {
        frames: &["", ".", "..", "..."],
        fps: 3,
    };
    pub const GLOBE: Spinner = Spinner {
        frames: &["🌍", "🌎", "🌏"],
        fps: 4,
    };
    pub const MOON: Spinner = Spinner {
        frames: &["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"],
        fps: 8,
    };
    pub const MONKEY: Spinner = Spinner {
        frames: &["🙈", "🙉", "🙊"],
        fps: 3,
    };
    pub const ARC: Spinner = Spinner {
        frames: &["◜", "◠", "◝", "◞", "◡", "◟"],
        fps: 7,
    };
    pub const NERD: Spinner = Spinner {
        frames: &["󰇙", "󰇙", "󰇙"],
        fps: 10,
    };
    pub const NERDARC: Spinner = Spinner {
        frames: &["◜", " ", "◝", "◞", "◡", "◟", " "],
        fps: 8,
    };
    pub const MINIDOT: Spinner = Spinner {
        frames: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
        fps: 12,
    };

    pub fn from_name(name: &str) -> &'static Spinner {
        match name {
            "line" => &Self::LINE,
            "jump" => &Self::JUMP,
            "pulse" => &Self::PULSE,
            "points" => &Self::POINTS,
            "meter" => &Self::METER,
            "hamburger" => &Self::HAMBURGER,
            "ellipsis" => &Self::ELLIPSIS,
            "globe" => &Self::GLOBE,
            "moon" => &Self::MOON,
            "monkey" => &Self::MONKEY,
            "arc" => &Self::ARC,
            "nerd" => &Self::NERD,
            "nerdarc" => &Self::NERDARC,
            "minidot" => &Self::MINIDOT,
            _ => &Self::DOT, // default
        }
    }

    pub fn current_frame(&self) -> &'static str {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let ms_per_frame = 1000 / (self.fps as u64);
        let current_frame = (ms / ms_per_frame) % (self.frames.len() as u64);
        self.frames[current_frame as usize]
    }
}
