use crate::traits::{Dflt, Edge, Generated, Made, Named, Remote, Req, Tr};
use crate::widget::{Gadget, Widget};
use std::fmt::Display;

impl Req for Widget {
    fn req(&self) -> u32 {
        1
    }
}

impl Req for Gadget {
    fn req(&self) -> u32 {
        2
    }
}

impl Dflt for Widget {
    fn dm(&self) -> u32 {
        3
    }
}

impl Dflt for Gadget {}

impl Named for Widget {
    fn name(&self) -> String {
        "trait".to_string()
    }
}

impl<T: Display> Tr for T {
    fn tr(&self) -> String {
        self.to_string()
    }
}

impl Edge for &Widget {
    fn edge(&self) -> u32 {
        4
    }
}

impl Edge for (u8, u8) {
    fn edge(&self) -> u32 {
        5
    }
}

impl Made for Gadget {
    made_method!();
}

impl Remote for Gadget {
    fn remote(&self) -> u32 {
        6
    }
}

generated_impl!(Gadget);
