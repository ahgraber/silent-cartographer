use std::fmt;

pub struct Widget;
pub struct Gadget;

impl Widget {
    pub fn name(&self) -> String {
        "inherent".to_string()
    }
}

impl fmt::Display for Widget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "widget")
    }
}
