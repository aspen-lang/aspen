use core::fmt;

pub struct Out;

impl fmt::Write for Out {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            libc::write(
                libc::STDOUT_FILENO,
                s.as_bytes() as *const [u8] as *const _,
                s.len(),
            );
        }
        Ok(())
    }
}

// macro_rules! print {
//     ($fmt:expr, $($args:tt)+) => {
//         core::fmt::write(&mut crate::print::Out, format_args!($fmt, $($args)+)).unwrap();
//     }
// }

macro_rules! println {
    ($fmt:expr) => {
        core::fmt::Write::write_str(
            &mut crate::print::Out,
            concat!($fmt, "\n"),
        ).unwrap();
    };

    ($fmt:expr, $($args:tt)*) => {
        core::fmt::write(&mut crate::print::Out, format_args!(concat!($fmt, "\n"), $($args)*)).unwrap();
    }
}
