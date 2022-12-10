use bytes::Bytes;

use std::borrow::Cow;
use std::io::IoSlice;
use std::ops::{Range, RangeBounds};

pub struct SpanIterator<'a>(&'a Span, usize);

impl<'a> Iterator for SpanIterator<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = match self.0 {
            Span::Many(spans) => spans.get(self.1).map(|b| &b[..]),
            Span::Single(bytes) => [bytes].get(self.1).map(|b| &b[..]),
            Span::Static(bytes) => [bytes].get(self.1).map(|b| &b[..]),
        };

        self.1 += 1;

        bytes
    }
}

/// A byte rope-like structure for efficiently appending and slicing byte sequences.
#[derive(Debug, Clone)]
pub enum Span {
    Many(Vec<Bytes>),
    Single(Bytes),
    Static(&'static [u8]),
}

impl FromIterator<Span> for Span {
    fn from_iter<I: IntoIterator<Item = Span>>(iter: I) -> Self {
        let mut new_spans = Vec::new();

        for span in iter {
            match span {
                Span::Many(spans) => new_spans.extend(spans.into_iter()),
                Span::Single(span) => new_spans.push(span.clone()),
                Span::Static(span) => new_spans.push(Bytes::from_static(span)),
            }
        }

        Span::Many(new_spans)
    }
}

impl From<&'static [u8]> for Span {
    fn from(bytes: &'static [u8]) -> Self {
        Span::Static(bytes)
    }
}

impl From<Vec<u8>> for Span {
    fn from(bytes: Vec<u8>) -> Self {
        Bytes::from(bytes).into()
    }
}

impl From<Bytes> for Span {
    fn from(bytes: Bytes) -> Self {
        Span::Single(bytes)
    }
}

impl Span {
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        match self {
            Span::Many(_) | Span::Static(_) => {
                use std::ops::Bound::*;

                let start = range.start_bound();
                let end = range.end_bound();

                let start = match start {
                    Unbounded => 0,
                    Included(&n) => n,
                    Excluded(&n) => n + 1,
                };
                let end = match end {
                    Unbounded => self.len(),
                    Included(&n) => n + 1,
                    Excluded(&n) => n,
                };

                self.slice_range(start..end)
            }
            Span::Single(bytes) => Span::Single(bytes.slice(range)),
        }
    }

    fn slice_range(&self, range: Range<usize>) -> Self {
        match self {
            Span::Many(spans) => {
                let mut new_spans = Vec::new();
                let mut i = 0;

                for span in spans {
                    if i >= range.end {
                        break;
                    }

                    if range.start >= i + span.len() {
                        i += span.len();
                        continue;
                    }

                    let slice_end = (range.end - i).min(span.len());
                    let slice_start = if range.start > i {
                        dbg!(&range.start, &i);
                        range.start - i
                    } else {
                        0
                    };

                    new_spans.push(span.slice(slice_start..slice_end));

                    i += span.len();
                }

                Span::Many(new_spans)
            }
            Span::Single(bytes) => Span::Single(bytes.slice(range)),
            Span::Static(bytes) => Span::Static(&bytes[range]),
        }
    }

    /// Converts the Span into a single contigious slice. If the span consists of multiple byte
    /// sequences it will first coalesce them into one slice.
    pub fn to_slice<'a>(&'a self) -> Cow<'a, [u8]> {
        match self {
            Span::Many(spans) => Cow::Owned(self.to_bytes().to_vec()),
            Span::Single(span) => Cow::Borrowed(span),
            Span::Static(span) => Cow::Borrowed(span),
        }
    }

    /// Returns an iterator over all the internal byte slices.
    pub fn spans<'a>(&'a self) -> SpanIterator<'a> {
        SpanIterator(self, 0)
    }

    pub fn to_io_slice<'a>(&'a self) -> Vec<IoSlice<'a>> {
        match self {
            Span::Many(spans) => spans.iter().map(|s| IoSlice::new(s)).collect::<Vec<_>>(),
            Span::Single(span) => vec![IoSlice::new(span)],
            Span::Static(span) => vec![IoSlice::new(span)],
        }
    }

    pub fn to_byte_spans(&self) -> Vec<Bytes> {
        match self {
            Span::Many(spans) => spans.clone(),
            Span::Single(span) => vec![span.clone()],
            &Span::Static(span) => vec![span.into()],
        }
    }
    /// Converts the span into a [Bytes].
    pub fn to_bytes(&self) -> Bytes {
        match self {
            Span::Many(bytes) => bytes
                .iter()
                .flat_map(|b| b.iter())
                .cloned()
                .collect::<Bytes>(),
            Span::Single(bytes) => bytes.clone(),
            Span::Static(bytes) => Bytes::from_static(bytes),
        }
    }

    /// The length of the span.
    pub fn len(&self) -> usize {
        match self {
            Span::Many(bytes) => bytes.iter().map(|b| b.len()).sum(),
            Span::Single(bytes) => bytes.len(),
            Span::Static(bytes) => bytes.len(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use test_case::test_case;

    #[test_case(&[b"abc", b"def", b"ghj"], .., b"abcdefghj")]
    #[test_case(&[b"abc", b"def", b"ghj"], 1..8, b"bcdefgh")]
    #[test_case(&[b"abc", b"def", b"ghj"], ..1, b"a")]
    #[test_case(&[b"abc", b"def", b"ghj"], 3..=6, b"defg")]
    #[test_case(&[b"abc", b"def", b"ghj"], 3..6, b"def")]
    #[test_case(&[b"a", b"def", b"j"], .., b"adefj")]
    #[test_case(&[b"a", b"def", b"j"], 1..4, b"def")]
    #[test_case(&[b"a", b"def", b"j"], 1.., b"defj")]
    #[test_case(&[b"a", b"def", b"j"], ..4, b"adef")]
    fn slice(spans: &[&'static [u8]], slice: impl RangeBounds<usize>, expected: &[u8]) {
        let span = spans.iter().map(|&s| Span::from(s)).collect::<Span>();
        dbg!(&span);
        let sliced_span = span.slice(slice);
        dbg!(&sliced_span);
        let bytes = sliced_span.to_bytes();

        assert_eq!(expected, bytes);
    }
}
