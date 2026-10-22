macro_rules! made_method {
    () => {
        fn made(&self) -> u32 {
            7
        }
    };
}

macro_rules! generated_impl {
    ($ty:ty) => {
        impl Generated for $ty {
            fn generated(&self) -> u32 {
                8
            }
        }
    };
}
