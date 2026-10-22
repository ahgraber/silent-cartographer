pub trait SharedTr {
    fn shared(&self) -> u32;
}

pub struct SharedType;

impl SharedTr for SharedType {
    fn shared(&self) -> u32 {
        8
    }
}
