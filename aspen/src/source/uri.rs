use std::convert::TryInto;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct URI {
    uri: String,
    scheme_len: usize,
}

impl URI {
    pub fn new<S, P>(scheme: S, path: P) -> URI
    where
        S: ToString,
        P: AsRef<str>,
    {
        let scheme = scheme.to_string();
        let scheme_len = scheme.len();
        let mut uri = scheme;
        uri.push(':');
        uri.push_str(path.as_ref());

        URI { uri, scheme_len }
    }

    pub fn file<P: AsRef<Path>>(path: P) -> URI {
        URI::new("file", format!("//{}", path.as_ref().display()))
    }

    pub fn stdin() -> URI {
        URI::new("std", "in")
    }

    fn scheme(&self) -> &str {
        &self.uri[..self.scheme_len]
    }

    fn path(&self) -> &str {
        &self.uri[self.scheme_len + 1..]
    }

    pub fn short_name(&self) -> &str {
        let mut index = self.scheme_len + 1;
        let path = self.path();
        let len = path.len();

        for (i, c) in path.chars().enumerate() {
            if c == '/' && i < len {
                index = i + self.scheme_len + 2;
            }
        }

        &self.uri[index..]
    }

    pub fn uri(&self) -> &str {
        self.uri.as_str()
    }
}

impl fmt::Debug for URI {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.scheme(), self.path())
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

        URI::new(scheme, path)
    }
}

impl TryInto<PathBuf> for &URI {
    type Error = io::Error;

    fn try_into(self) -> Result<PathBuf, Self::Error> {
        if self.scheme() != "file" {
            Err(io::ErrorKind::PermissionDenied.into())
        } else {
            let mut path = self.path().to_string();
            path.remove(0);
            path.remove(0);
            PathBuf::from(path).canonicalize()
        }
    }
}

impl AsRef<str> for &URI {
    fn as_ref(&self) -> &str {
        &self.uri
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
