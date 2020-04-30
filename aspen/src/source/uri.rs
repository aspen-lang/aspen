use std::fmt;
use std::path::Path;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct URI {
    scheme: String,
    path: String,
}

impl URI {
    pub fn new<S, P>(scheme: S, path: P) -> URI
    where
        S: ToString,
        P: ToString,
    {
        URI {
            scheme: scheme.to_string(),
            path: path.to_string(),
        }
    }

    pub fn file<P: AsRef<Path>>(path: P) -> URI {
        URI::new("file", format!("//{}", path.as_ref().display()))
    }

    pub fn stdin() -> URI {
        URI::new("std", "in")
    }

    pub fn short_name(&self) -> &str {
        let mut index = 0;
        let len = self.path.len();

        for (i, c) in self.path.chars().enumerate() {
            if c == '/' && i < len {
                index = i + 1;
            }
        }

        &self.path[index..]
    }
}

impl fmt::Debug for URI {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.scheme, self.path)
    }
}

impl fmt::Display for URI {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.short_name())
    }
}

impl<'a> From<&'a str> for URI {
    fn from(s: &'a str) -> Self {
        let mut scheme = String::new();
        let mut path = String::new();

        let mut chars = s.chars();
        loop {
            match chars.next() {
                None => break,
                Some(':') => {
                    path = chars.collect();
                    break;
                }
                Some(c) => scheme.push(c),
            }
        }

        URI { scheme, path }
    }
}

#[test]
fn short_name() {
    assert_eq!("", URI::from("").short_name());
    assert_eq!("", URI::from(":").short_name());
    assert_eq!("a", URI::from(":a").short_name());
    assert_eq!("", URI::from("a:").short_name());
    assert_eq!("b", URI::from(":a/b").short_name());
    assert_eq!("", URI::from(":a/b/").short_name());
}
