/// A trait with a required method.
pub trait Req {
    fn req(&self) -> u32;
}

/// A trait with a default method.
pub trait Dflt {
    fn dm(&self) -> u32 {
        0
    }
}

/// A trait whose method shares a name with an inherent method of `Widget`.
pub trait Named {
    fn name(&self) -> String;
}

/// A trait implemented for every `Display` type.
pub trait Tr {
    fn tr(&self) -> String;
}

/// A trait implemented for `&Widget` and `(u8, u8)`.
pub trait Edge {
    fn edge(&self) -> u32;
}

/// A trait whose method a macro generates inside an impl.
pub trait Made {
    fn made(&self) -> u32;
}

/// A trait implemented in another file.
pub trait Remote {
    fn remote(&self) -> u32;
}

/// A trait whose whole impl a macro generates.
pub trait Generated {
    fn generated(&self) -> u32;
}
