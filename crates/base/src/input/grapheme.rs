//! Extended-grapheme boundaries over borrowed rope chunks.
use ropey::Rope;
use sum_tree::Bias;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

use super::RopeExt as _;

pub(super) fn previous(text: &Rope, offset: usize) -> usize {
    let offset = text.clip_offset(offset, Bias::Left);
    query(
        text,
        offset,
        offset.saturating_sub(1),
        |cursor, chunk, start| cursor.prev_boundary(chunk, start),
    )
    .unwrap_or(0)
}

pub(super) fn next(text: &Rope, offset: usize) -> usize {
    let offset = text.clip_offset(offset, Bias::Right);
    query(text, offset, offset, |cursor, chunk, start| {
        cursor.next_boundary(chunk, start)
    })
    .unwrap_or(text.len())
}

pub(super) fn floor(text: &Rope, offset: usize) -> usize {
    let offset = text.clip_offset(offset, Bias::Left);
    if is_boundary(text, offset) {
        offset
    } else {
        previous(text, offset)
    }
}

pub(super) fn ceil(text: &Rope, offset: usize) -> usize {
    let offset = text.clip_offset(offset, Bias::Right);
    if is_boundary(text, offset) {
        offset
    } else {
        next(text, offset)
    }
}

fn is_boundary(text: &Rope, offset: usize) -> bool {
    query(text, offset, offset, |cursor, chunk, start| {
        cursor.is_boundary(chunk, start)
    })
}

fn query<T>(
    text: &Rope,
    offset: usize,
    chunk_offset: usize,
    mut operation: impl FnMut(&mut GraphemeCursor, &str, usize) -> Result<T, GraphemeIncomplete>,
) -> T {
    let mut cursor = GraphemeCursor::new(offset, text.len(), true);
    let (mut chunk, mut start) = text.chunk(chunk_offset);
    loop {
        match operation(&mut cursor, chunk, start) {
            Ok(result) => return result,
            Err(GraphemeIncomplete::PreContext(end)) => {
                // Context can extend arbitrarily far back (e.g. regional indicators).
                // Supply exactly the requested suffix endpoint without joining chunks.
                let (context, context_start) = text.chunk(end - 1);
                cursor.provide_context(&context[..end - context_start], context_start);
            }
            Err(GraphemeIncomplete::PrevChunk) => (chunk, start) = text.chunk(start - 1),
            Err(GraphemeIncomplete::NextChunk) => (chunk, start) = text.chunk(start + chunk.len()),
            Err(GraphemeIncomplete::InvalidOffset) => {
                unreachable!("grapheme cursor and rope chunk offsets must agree")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_segmentation::UnicodeSegmentation;

    fn assert_boundaries(value: &str, multi_chunk: bool) {
        let text = Rope::from(value);
        if multi_chunk {
            assert!(text.chunks().count() > 2, "fixture must cross rope chunks");
        }
        let boundaries: Vec<_> = value
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain([value.len()])
            .collect();
        for offset in (0..=value.len()).chain([usize::MAX]) {
            let left = text.clip_offset(offset, Bias::Left);
            let right = text.clip_offset(offset, Bias::Right);
            let expected_previous = boundaries
                .iter()
                .copied()
                .filter(|i| *i < left)
                .next_back()
                .unwrap_or(0);
            let expected_next = boundaries
                .iter()
                .copied()
                .find(|i| *i > right)
                .unwrap_or(value.len());
            assert_eq!(
                previous(&text, offset),
                expected_previous,
                "previous at {offset}"
            );
            assert_eq!(next(&text, offset), expected_next, "next at {offset}");
            assert_eq!(
                floor(&text, offset),
                *boundaries
                    .iter()
                    .filter(|i| **i <= left)
                    .next_back()
                    .unwrap(),
                "floor at {offset}"
            );
            assert_eq!(
                ceil(&text, offset),
                *boundaries.iter().find(|i| **i >= right).unwrap(),
                "ceil at {offset}"
            );
        }
    }

    #[test]
    fn grapheme_boundaries_match_string_oracle() {
        for text in ["", "a中🎉e\u{301}👍🏽🇺🇸👨‍👩‍👧‍👦", "\r\n\rX\n"] {
            assert_boundaries(text, false);
        }
    }

    #[test]
    fn grapheme_combining_and_zwj_sequences_cross_chunks() {
        let extended_before_joiner = format!("a👩{}\u{200d}👩b", "\u{301}".repeat(1200));
        assert_boundaries(&extended_before_joiner, true);
        let combining = format!("ae{}b", "\u{301}".repeat(1200));
        assert_boundaries(&combining, true);
        let joined = format!("a👩{}b", "\u{301}\u{200d}👩".repeat(350));
        assert_boundaries(&joined, true);
    }

    #[test]
    fn grapheme_regional_indicator_context_crosses_chunks() {
        assert_boundaries(&format!("a{}b", "🇺".repeat(801)), true);
    }
}
