pub trait Merge where Self: Sized {
    fn merge<I: IntoIterator<Item=Self>>(all: I) -> Self;
}

impl Merge for () {
    fn merge<I: IntoIterator<Item=()>>(all: I) -> () {
        all.into_iter().for_each(drop);
    }
}
