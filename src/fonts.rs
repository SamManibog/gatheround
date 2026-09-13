/// the minimum set of characters that every font will try to load
pub const BASE_CHARSET: &str = "!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

#[derive(Debug)]
pub struct BuiltinFontData {
    pub name: &'static str,
    pub filetype: &'static str,
    pub data: &'static [u8],
}

pub const JETBRAINS_MONO: BuiltinFontData = BuiltinFontData {
    name: "jetbrains-mono.ttf",
    filetype: ".ttf",
    data: include_bytes!("jetbrains-mono.ttf"),
};

pub const PT_SERIF_REGULAR: BuiltinFontData = BuiltinFontData {
    name: "ptserif.ttf",
    filetype: ".ttf",
    data: include_bytes!("ptserif.ttf"),
};

pub const PT_SERIF_BOLD: BuiltinFontData = BuiltinFontData {
    name: "ptserif-bold.ttf",
    filetype: ".ttf",
    data: include_bytes!("ptserif-bold.ttf"),
};

impl BuiltinFontData {
    /// gets builtin font data by name
    pub fn get_by_name(font_name: &str) -> Option<&'static Self> {
        match font_name {
            "jetbrains-mono.ttf" => Some(&JETBRAINS_MONO),
            "ptserif.ttf" => Some(&PT_SERIF_REGULAR),
            "ptserif-bold.ttf" => Some(&PT_SERIF_BOLD),
            _ => None,
        }
    }
}
