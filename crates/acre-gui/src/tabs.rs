//! The bookkeeping of the drawings open in tabs: which tab holds a
//! file, where a new drawing goes, and which tab is shown after one
//! closes.

use std::path::Path;

use crate::document::Document;

/// The index of the drawing in `docs` whose file is `path`, or of an
/// untitled drawing when `path` is `None`.
pub fn find(docs: &[Document], path: Option<&Path>) -> Option<usize> {
    docs.iter()
        .position(|d| !d.blank && d.path.as_deref() == path)
}

/// Add `doc` to `docs` and give its index. It takes the place of a blank
/// drawing, and goes after the last tab otherwise.
pub fn add(docs: &mut Vec<Document>, doc: Document) -> usize {
    if let [only] = &mut docs[..]
        && only.blank
    {
        *only = doc;
        return 0;
    }
    docs.push(doc);
    docs.len() - 1
}

/// Close the tab at `closed`, and give the index of the tab shown after
/// it when the one at `active` was shown. Closing the shown tab shows
/// the tab that takes its place, or the last tab when it was the last.
/// Closing the only tab leaves a blank drawing.
pub fn close(docs: &mut Vec<Document>, active: usize, closed: usize) -> usize {
    docs.remove(closed);
    if docs.is_empty() {
        docs.push(Document::blank());
        return 0;
    }
    if closed < active {
        active - 1
    } else {
        active.min(docs.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn at(path: &str) -> Document {
        let mut doc = Document::new();
        doc.path = Some(PathBuf::from(path));
        doc
    }

    fn paths(docs: &[Document]) -> Vec<Option<&str>> {
        docs.iter()
            .map(|d| d.path.as_deref().and_then(Path::to_str))
            .collect()
    }

    #[test]
    fn the_first_drawing_takes_the_blank_ones_place() {
        let mut docs = vec![Document::blank()];
        assert_eq!(find(&docs, None), None);
        assert_eq!(add(&mut docs, at("/a.acre")), 0);
        assert_eq!(add(&mut docs, at("/b.acre")), 1);
        assert_eq!(paths(&docs), [Some("/a.acre"), Some("/b.acre")]);
    }

    #[test]
    fn a_drawing_is_found_by_its_file() {
        let mut docs = vec![at("/a.acre"), at("/b.acre")];
        assert_eq!(find(&docs, Some(Path::new("/b.acre"))), Some(1));
        assert_eq!(find(&docs, Some(Path::new("/c.acre"))), None);
        assert_eq!(find(&docs, None), None);
        add(&mut docs, Document::new());
        assert_eq!(find(&docs, None), Some(2));
    }

    #[test]
    fn closing_a_tab_keeps_the_shown_drawing_shown() {
        let mut docs = vec![at("/a.acre"), at("/b.acre"), at("/c.acre")];
        assert_eq!(close(&mut docs, 2, 0), 1);
        assert_eq!(paths(&docs), [Some("/b.acre"), Some("/c.acre")]);
        let mut docs = vec![at("/a.acre"), at("/b.acre"), at("/c.acre")];
        assert_eq!(close(&mut docs, 0, 2), 0);
    }

    #[test]
    fn closing_the_shown_tab_shows_the_next_or_else_the_last() {
        let mut docs = vec![at("/a.acre"), at("/b.acre"), at("/c.acre")];
        assert_eq!(close(&mut docs, 1, 1), 1);
        assert_eq!(paths(&docs), [Some("/a.acre"), Some("/c.acre")]);
        assert_eq!(close(&mut docs, 1, 1), 0);
        assert_eq!(paths(&docs), [Some("/a.acre")]);
    }

    #[test]
    fn closing_the_last_tab_leaves_a_blank_drawing() {
        let mut docs = vec![at("/a.acre")];
        assert_eq!(close(&mut docs, 0, 0), 0);
        assert!(docs.len() == 1 && docs[0].blank);
        assert_eq!(add(&mut docs, at("/b.acre")), 0);
        assert_eq!(paths(&docs), [Some("/b.acre")]);
    }
}
