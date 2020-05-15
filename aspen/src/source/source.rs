use crate::source::{Location, URI};
use crate::Range;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs::File;
use tokio::io::{stdin, AsyncRead, AsyncReadExt};
use unicode_segmentation::UnicodeSegmentation;

pub struct Source {
    uri: URI,
    code: String,
    len: usize,
    offset_byte_indices: HashMap<usize, usize>,
    line_breaks: Vec<usize>,
    pub modified: SystemTime,
    pub kind: SourceKind,
}

#[derive(Debug)]
pub enum SourceKind {
    Module,
    Inline,
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({:?})", self.uri, self.kind)
    }
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.uri == other.uri
    }
}

impl Eq for Source {}

impl Hash for Source {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uri.hash(state)
    }
}

impl Source {
    pub fn new<U, C>(uri: U, code: C) -> Arc<Source>
    where
        U: Into<URI>,
        C: Into<String>,
    {
        Self::create(
            uri.into(),
            code.into(),
            SystemTime::now(),
            SourceKind::Module,
        )
    }

    pub fn inline<U, C>(uri: U, code: C) -> Arc<Source>
    where
        U: Into<URI>,
        C: Into<String>,
    {
        Self::create(
            uri.into(),
            code.into(),
            SystemTime::now(),
            SourceKind::Inline,
        )
    }

    pub async fn read<U, R>(uri: U, read: R) -> io::Result<Arc<Source>>
    where
        U: Into<URI>,
        R: AsyncRead + Unpin,
    {
        Self::create_read(uri.into(), read, SystemTime::now()).await
    }

    pub async fn file<P: AsRef<Path>>(path: P) -> io::Result<Arc<Source>> {
        let path = path.as_ref().canonicalize()?;
        let uri = URI::file(&path);
        let file = File::open(path).await?;
        let modified = file.metadata().await?.modified()?;

        Self::create_read(uri, file, modified).await
    }

    pub async fn files<P: AsRef<str>>(pattern: P) -> Vec<Arc<Source>> {
        if let Ok(paths) = glob::glob(pattern.as_ref()) {
            futures::future::join_all(paths.into_iter().filter_map(Result::ok).map(Self::file))
                .await
                .into_iter()
                .filter_map(Result::ok)
                .collect()
        } else {
            vec![]
        }
    }

    pub async fn stdin() -> io::Result<Arc<Source>> {
        Self::read(URI::stdin(), stdin()).await
    }

    async fn create_read<R>(uri: URI, mut read: R, modified: SystemTime) -> io::Result<Arc<Source>>
    where
        R: AsyncRead + Unpin,
    {
        let mut code = String::new();
        read.read_to_string(&mut code).await?;
        Ok(Self::create(uri, code, modified, SourceKind::Module))
    }

    fn create(uri: URI, code: String, modified: SystemTime, kind: SourceKind) -> Arc<Source> {
        let mut offset = 0;
        let mut offset_byte_indices = HashMap::new();
        let mut line_breaks = vec![];

        for (byte_offset, grapheme) in code.grapheme_indices(true) {
            if grapheme == "\n" {
                line_breaks.push(offset);
            }

            offset_byte_indices.insert(offset, byte_offset);
            offset += 1
        }

        offset_byte_indices.insert(offset, code.len());

        Arc::new(Source {
            uri,
            code,
            len: offset,
            offset_byte_indices,
            line_breaks,
            modified,
            kind,
        })
    }

    pub fn graphemes(&self) -> Graphemes {
        Graphemes::new(&self.code[..], &self.offset_byte_indices)
    }

    pub fn uri(&self) -> &URI {
        &self.uri
    }

    pub fn short_name(&self) -> &str {
        self.uri.short_name()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn location_at(&self, offset: usize) -> Location {
        let mut line = 1;
        let mut character = offset + 1;
        for line_break_offset in &self.line_breaks {
            if offset <= *line_break_offset {
                break;
            }
            character = offset - *line_break_offset;
            line += 1;
        }

        Location {
            line,
            character,
            offset,
        }
    }

    pub fn location_at_coords(&self, line: usize, character: usize) -> Location {
        let offset = if line == 1 {
            character - 1
        } else {
            self.line_breaks[line - 2] + character
        };

        Location {
            offset,
            line,
            character,
        }
    }

    pub fn slice<R: Into<std::ops::Range<usize>>>(&self, range: R) -> &str {
        let range = range.into();
        if range.end > self.len {
            panic!("offset out of range");
        }

        let start_byte_offset = *self.offset_byte_indices.get(&range.start).unwrap();
        let end_byte_offset = *self.offset_byte_indices.get(&range.end).unwrap();

        let length = end_byte_offset - start_byte_offset;

        let ptr = self.code[start_byte_offset..].as_ptr();

        unsafe { std::str::from_utf8(std::slice::from_raw_parts(ptr, length)).unwrap() }
    }

    pub fn eof_location(&self) -> Location {
        Location {
            offset: self.len,
            line: self.line_breaks.len() + 1,
            character: self
                .line_breaks
                .last()
                .map(|b| self.len - *b)
                .unwrap_or(self.len),
        }
    }

    pub fn eof_range(&self) -> Range {
        let location = self.eof_location();
        Range {
            start: location.clone(),
            end: location,
        }
    }

    pub fn range_all(&self) -> Range {
        Range {
            start: Location {
                offset: 0,
                line: 1,
                character: 1,
            },
            end: self.eof_location(),
        }
    }

    pub fn apply_edits<I: IntoIterator<Item = (Option<Range>, String)>>(
        &self,
        edits: I,
    ) -> Arc<Source> {
        let range_all = self.range_all();
        let mut edits = edits
            .into_iter()
            .map(|(r, s)| (r.unwrap_or(range_all.clone()), s))
            .collect::<Vec<_>>();
        edits.sort_by_key(|(range, _)| range.start.offset);

        let mut new_code = String::new();
        let mut offset = 0;
        for (range, text) in edits {
            new_code.push_str(self.slice(offset..range.start.offset));
            new_code.push_str(text.as_str());
            offset = range.end.offset;
        }
        new_code.push_str(self.slice(offset..self.len));

        Self::new(self.uri.clone(), new_code)
    }
}

pub struct Graphemes<'a> {
    code: &'a str,
    offset_byte_indices: &'a HashMap<usize, usize>,
    offset: usize,
}

impl<'a> Graphemes<'a> {
    pub fn new(code: &'a str, offset_byte_indices: &'a HashMap<usize, usize>) -> Graphemes<'a> {
        Graphemes {
            code,
            offset_byte_indices,
            offset: 0,
        }
    }
}

impl<'a> Iterator for Graphemes<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        let byte_index = *self.offset_byte_indices.get(&self.offset)?;

        if byte_index == self.code.len() {
            return None;
        }

        let next_index = self.offset_byte_indices.get(&(self.offset + 1));

        let item = match next_index {
            None => (self.offset, &self.code[byte_index..]),
            Some(end_index) => (self.offset, &self.code[byte_index..*end_index]),
        };

        self.offset += 1;

        Some(item)
    }
}
