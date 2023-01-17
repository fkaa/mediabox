use bytes::{Bytes, BytesMut};

use std::borrow::Cow;
use std::io::IoSlice;
use std::ops::{Range, RangeBounds};
use std::sync::Arc;

use crate::memory::Memory;

/// A byte rope-like structure for efficiently appending and slicing byte sequences.
#[derive(Debug, Clone)]
pub enum Span<'a> {
    Many(Vec<Span<'a>>),
    Single(Bytes),
    Slice(&'a [u8]),
    RefCounted(Option<Arc<Memory>>, usize, usize),
}

impl<'a> Default for Span<'a> {
    fn default() -> Span<'static> {
        Span::Slice(&[])
    }
}

impl<'a> FromIterator<Span<'a>> for Span<'a> {
    fn from_iter<I: IntoIterator<Item = Span<'a>>>(iter: I) -> Self {
        Span::Many(iter.into_iter().collect())
    }
}

impl From<()> for Span<'static> {
    fn from(_: ()) -> Self {
        Span::Slice(&[])
    }
}

impl From<&'static [u8]> for Span<'static> {
    fn from(bytes: &'static [u8]) -> Self {
        Span::Slice(bytes)
    }
}

impl From<Vec<u8>> for Span<'static> {
    fn from(bytes: Vec<u8>) -> Self {
        Bytes::from(bytes).into()
    }
}

impl From<Bytes> for Span<'static> {
    fn from(bytes: Bytes) -> Self {
        Span::Single(bytes)
    }
}

impl Span<'static> {
    pub fn visit_bytes<F: FnMut(Bytes)>(&self, func: &mut F) {
        match self {
            Span::Many(spans) => {
                for span in spans {
                    span.visit_bytes(func);
                }
            }
            Span::Single(span) => func(span.clone()),
            Span::Slice(span) => func(Bytes::from_static(span)),
        }
    }

    pub fn to_byte_spans(&self) -> Vec<Bytes> {
        let mut bytes = Vec::new();

        self.visit_bytes(&mut |b| bytes.push(b));

        bytes
    }
}

impl<'a> Span<'a> {
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        match self {
            Span::Many(_) | Span::Slice(_) | Span::RefCounted(...) => {
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
            Span::Slice(bytes) => Span::Slice(&bytes[range]),
        }
    }

    /// Converts the Span into a single contigious slice. If the span consists of multiple byte
    /// sequences it will first coalesce them into one slice.
    pub fn to_slice(&'a self) -> Cow<'a, [u8]> {
        match self {
            Span::Many(spans) => Cow::Owned(self.to_bytes().to_vec()),
            Span::Single(span) => Cow::Borrowed(span),
            Span::Slice(span) => Cow::Borrowed(span),
        }
    }

    pub fn visit<F: FnMut(&'a [u8])>(&'a self, func: &mut F) {
        match self {
            Span::Many(spans) => {
                for span in spans {
                    span.visit(func);
                }
            }
            Span::Single(span) => func(&span[..]),
            Span::Slice(span) => func(&span[..]),
        }
    }

    pub fn to_io_slice(&'a self) -> Vec<IoSlice<'a>> {
        let mut slices = Vec::new();
        self.visit(&mut |b| slices.push(IoSlice::new(b)));
        slices
    }

    /// Converts the span into one contiguous [Bytes].
    pub fn to_bytes(&self) -> Bytes {
        match self {
            Span::Many(bytes) => {
                let mut bytes = BytesMut::new();
                self.visit(&mut |b| bytes.extend(&b[..]));
                bytes.freeze()
            }
            Span::Single(bytes) => bytes.clone(),
            Span::Slice(bytes) => Bytes::copy_from_slice(bytes),
        }
    }

    /// The length of the span.
    pub fn len(&self) -> usize {
        let mut len = 0;

        self.visit(&mut |b| len += b.len());

        len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
