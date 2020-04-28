use crate::source::{Location, URI};
use std::sync::Arc;
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

pub struct Source {
    uri: URI,
    code: String,
    len: usize,
    offset_byte_indices: HashMap<usize, usize>,
    line_breaks: Vec<usize>,
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.uri == other.uri
    }
}

impl Source {
    pub fn new<U, C>(uri: U, code: C) -> Arc<Source>
    where
        U: Into<URI>,
        C: Into<String>,
    {
        let code = code.into();

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
            uri: uri.into(),
            code,
            len: offset,
            offset_byte_indices,
            line_breaks,
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
        let mut character = 1;
        for line_break_offset in &self.line_breaks {
            if offset < *line_break_offset {
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

    pub fn slice<R: Into<std::ops::Range<usize>>>(&self, range: R) -> &str {
        let range = range.into();
        if range.start >= self.len {
            panic!("offset out of range");
        }

        let start_byte_offset = *self.offset_byte_indices.get(&range.start).unwrap();
        let end_byte_offset = *self.offset_byte_indices.get(&range.end).unwrap();

        let length = end_byte_offset - start_byte_offset;

        let ptr = self.code[start_byte_offset..].as_ptr();

        unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(ptr, length))
                .unwrap()
        }
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
