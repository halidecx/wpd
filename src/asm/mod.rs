/* A hand-written assembly symbol reaches Rust as a marker type, so that the
 * dispatch ladders and the safe wrappers can name one by type rather than by
 * function pointer. The extern declaration spells the argument types out --
 * that is what checks each symbol against the signature it is installed as. */
macro_rules! raw {
    ($marker:ident, $inner:ident, $sig:ty, $sym:literal, ($($arg:ty),* $(,)?)) => {
        unsafe extern "C" {
            #[link_name = $sym]
            fn $inner($(_: $arg),*);
        }

        pub struct $marker;

        impl Raw for $marker {
            type Sig = $sig;
            const F: $sig = $inner;
        }
    };
}

pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
pub mod yuv;

pub(crate) trait Raw {
    type Sig: Copy;
    const F: Self::Sig;
}
