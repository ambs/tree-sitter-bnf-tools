//! [`Reflect`]: one read-only view of every node of every generated typed
//! tree, and [`walk`], the iterative traversal that goes with it.

use crate::{ErrorRegion, Span, Token};

/// A uniform, read-only view of any node in the typed tree, so one generic
/// loop — a serializer, a pretty-printer, a visitor — can walk the whole
/// thing without a function per type. Every generated struct implements it;
/// every generated enum implements it by delegating to the variant it
/// holds; so does [`Token`].
pub trait Reflect {
    /// The node's tree-sitter kind: the grammar rule's name (or alias),
    /// not the Rust type's name. For a [`Token`], the literal's kind.
    fn kind(&self) -> &str;

    /// Where the node is in the source.
    fn span(&self) -> &Span;

    /// The node's source text: `Some` only for a leaf (a kind with no
    /// children of its own), a [`Token`] or an [`ErrorRegion`]; `None` for
    /// a node whose text is made up of its children.
    fn text(&self) -> Option<&str>;

    /// The node's children in source order, each as a pair: the grammar
    /// field label it sits under (`None` for an unlabelled child, a literal
    /// token or a broken region), and the node. Labelled fields and
    /// unlabelled children come together, literal tokens included. An enum
    /// is transparent here: what comes back is the node the enum holds,
    /// never the enum itself. The label is the grammar's own (`target`, not
    /// the Rust identifier `r#type` a keyword label would need), and is the
    /// same one tree-sitter reports for that child.
    fn children(&self) -> Vec<(Option<&'static str>, &dyn Reflect)>;

    /// `true` for a node of a named rule — and for an [`ErrorRegion`], as in
    /// tree-sitter — `false` for a literal [`Token`].
    fn is_named(&self) -> bool;
}

impl Reflect for Token {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn span(&self) -> &Span {
        &self.span
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }

    fn children(&self) -> Vec<(Option<&'static str>, &dyn Reflect)> {
        Vec::new()
    }

    fn is_named(&self) -> bool {
        false
    }
}

impl Reflect for ErrorRegion {
    fn kind(&self) -> &str {
        "ERROR"
    }

    fn span(&self) -> &Span {
        &self.span
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }

    fn children(&self) -> Vec<(Option<&'static str>, &dyn Reflect)> {
        Vec::new()
    }

    fn is_named(&self) -> bool {
        true
    }
}

/// One node met by [`walk`]: the node itself, the grammar field label it sits
/// under in its parent (`None` for the root, for an unlabelled child, a
/// literal token or a broken region), and how deep it is (the root is 0).
#[derive(Clone, Copy)]
pub struct Visit<'a> {
    /// The node.
    pub node: &'a dyn Reflect,
    /// The field label it sits under in its parent, if any.
    pub field: Option<&'static str>,
    /// Its depth below the root: 0 for the root itself.
    pub depth: usize,
}

/// Walks `root` and everything below it, depth first, in source order — a
/// node before its children — without recursion: an explicit stack on the
/// heap, so a tree of any depth is walked without growing the call stack.
/// This is the traversal every consumer of [`Reflect`] would otherwise
/// write for itself; the depth and field label of each node are what is
/// needed to rebuild the tree's shape from the flat sequence.
pub fn walk(root: &dyn Reflect) -> Walk<'_> {
    Walk {
        stack: vec![(None, root, 0)],
    }
}

/// The iterator [`walk`] returns.
pub struct Walk<'a> {
    /// What is still to visit, the next node last: each with the field label
    /// it sits under and its depth.
    stack: Vec<(Option<&'static str>, &'a dyn Reflect, usize)>,
}

impl<'a> Iterator for Walk<'a> {
    type Item = Visit<'a>;

    fn next(&mut self) -> Option<Visit<'a>> {
        let (field, node, depth) = self.stack.pop()?;
        // Pushed in reverse, so the first child is the next one popped.
        for (child_field, child) in node.children().into_iter().rev() {
            self.stack.push((child_field, child, depth + 1));
        }
        Some(Visit { node, field, depth })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Pragma;
    use std::sync::OnceLock;

    /// A span from byte `start` to byte `end` on line 1, which is all these
    /// tests need of one.
    fn span(start: usize, end: usize) -> Span {
        Span {
            byte_range: start..end,
            start: Pragma {
                line: 1,
                column: start + 1,
            },
            end: Pragma {
                line: 1,
                column: end + 1,
            },
        }
    }

    /// The nodes of one toy tree, which every [`Toy`] points back at so that
    /// `children()` can hand out references to its siblings in the arena.
    type Arena = OnceLock<Vec<Toy>>;

    /// A node of a toy tree kept in a flat arena rather than as nested boxes,
    /// so that building and dropping a very deep tree is not itself recursive:
    /// the only recursion a test could trip over is the one under test.
    struct Toy {
        /// The arena this node lives in.
        arena: &'static Arena,
        /// What `kind()` returns.
        kind: &'static str,
        /// What `text()` returns.
        text: Option<&'static str>,
        /// Where `span()` says the node is.
        span: Span,
        /// The children, as the field label and the index in the arena.
        kids: Vec<(Option<&'static str>, usize)>,
    }

    impl Reflect for Toy {
        fn kind(&self) -> &str {
            self.kind
        }

        fn span(&self) -> &Span {
            &self.span
        }

        fn text(&self) -> Option<&str> {
            self.text
        }

        fn children(&self) -> Vec<(Option<&'static str>, &dyn Reflect)> {
            let all = self
                .arena
                .get()
                .expect("the arena is filled before it is used");
            let mut out = Vec::new();
            for &(field, index) in &self.kids {
                out.push((field, &all[index] as &dyn Reflect));
            }
            out
        }

        fn is_named(&self) -> bool {
            self.text.is_none() || self.kind.chars().all(char::is_alphabetic)
        }
    }

    /// A fresh, empty arena that lives for the rest of the process (a test's
    /// leak, so nodes can point back at it).
    fn arena() -> &'static Arena {
        Box::leak(Box::new(OnceLock::new()))
    }

    /// Builds this tree, the first node being the root:
    ///
    /// ```text
    /// file
    ///   "("                      (no field)
    ///   name: ident "x"
    ///   list                     (no field)
    ///     item: ident "a"
    ///     item: ident "b"
    ///   ")"                      (no field)
    /// ```
    fn small_tree() -> &'static Toy {
        let arena = arena();
        let node = |kind, text, start, end, kids| Toy {
            arena,
            kind,
            text,
            span: span(start, end),
            kids,
        };
        let nodes = vec![
            node(
                "file",
                None,
                0,
                9,
                vec![(None, 1), (Some("name"), 2), (None, 3), (None, 6)],
            ),
            node("(", Some("("), 0, 1, vec![]),
            node("ident", Some("x"), 1, 2, vec![]),
            node(
                "list",
                None,
                2,
                8,
                vec![(Some("item"), 4), (Some("item"), 5)],
            ),
            node("ident", Some("a"), 3, 4, vec![]),
            node("ident", Some("b"), 5, 6, vec![]),
            node(")", Some(")"), 8, 9, vec![]),
        ];
        arena.set(nodes).ok().expect("filled once");
        &arena.get().unwrap()[0]
    }

    /// Everything a visit carries, flattened to something comparable.
    fn shape(root: &dyn Reflect) -> Vec<(usize, Option<&'static str>, String)> {
        let mut out = Vec::new();
        for visit in walk(root) {
            out.push((visit.depth, visit.field, visit.node.kind().to_string()));
        }
        out
    }

    /// A node comes before its children, children in source order, and each
    /// visit carries the depth and the field label the node sits under.
    #[test]
    fn walk_is_pre_order_with_depth_and_field_labels() {
        let expected: Vec<(usize, Option<&str>, &str)> = vec![
            (0, None, "file"),
            (1, None, "("),
            (1, Some("name"), "ident"),
            (1, None, "list"),
            (2, Some("item"), "ident"),
            (2, Some("item"), "ident"),
            (1, None, ")"),
        ];
        let got = shape(small_tree());
        let got: Vec<(usize, Option<&str>, &str)> =
            got.iter().map(|(d, f, k)| (*d, *f, k.as_str())).collect();
        assert_eq!(got, expected);
    }

    /// Siblings come out in the order `children()` gives them, not reversed:
    /// the stack the walk uses must not flip them.
    #[test]
    fn walk_keeps_siblings_in_order() {
        let mut texts = Vec::new();
        for visit in walk(small_tree()) {
            if let Some(text) = visit.node.text() {
                texts.push(text.to_string());
            }
        }
        assert_eq!(texts, ["(", "x", "a", "b", ")"]);
    }

    /// A tree that is only a root yields just it, at depth 0 with no field.
    #[test]
    fn walk_of_a_lone_node_yields_only_the_root() {
        let arena = arena();
        arena
            .set(vec![Toy {
                arena,
                kind: "leaf",
                text: Some("z"),
                span: span(0, 1),
                kids: vec![],
            }])
            .ok()
            .expect("filled once");
        let visits = shape(&arena.get().unwrap()[0]);
        assert_eq!(visits, [(0, None, "leaf".to_string())]);
    }

    /// The walk is iterative: a chain a hundred thousand levels deep is walked
    /// on a thread whose stack could not hold even a thousand recursive calls.
    #[test]
    fn walk_does_not_recurse() {
        const DEPTH: usize = 100_000;
        let arena = arena();
        let mut nodes = Vec::new();
        for i in 0..DEPTH {
            let kids = if i + 1 < DEPTH {
                vec![(Some("next"), i + 1)]
            } else {
                vec![]
            };
            nodes.push(Toy {
                arena,
                kind: "link",
                text: None,
                span: span(i, i + 1),
                kids,
            });
        }
        arena.set(nodes).ok().expect("filled once");
        let root: &'static Toy = &arena.get().unwrap()[0];

        let (visited, deepest) = std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || {
                let (mut visited, mut deepest) = (0usize, 0usize);
                for visit in walk(root) {
                    visited += 1;
                    deepest = deepest.max(visit.depth);
                }
                (visited, deepest)
            })
            .expect("spawn")
            .join()
            .expect("walking must not overflow a small stack");
        assert_eq!(visited, DEPTH);
        assert_eq!(deepest, DEPTH - 1);
    }

    /// A literal is an unnamed leaf: its kind is the literal, its text the
    /// source, and it has nothing below it.
    #[test]
    fn a_token_reflects_as_an_unnamed_leaf() {
        let token = Token {
            kind: "=>".into(),
            text: "=>".into(),
            span: span(4, 6),
        };
        assert_eq!(token.kind(), "=>");
        assert_eq!(token.text(), Some("=>"));
        assert_eq!(token.span().byte_range, 4..6);
        assert!(!token.is_named());
        assert!(token.children().is_empty());
    }

    /// A broken region is a named leaf of kind `ERROR` — as in tree-sitter —
    /// holding the text it covers.
    #[test]
    fn an_error_region_reflects_as_a_named_error_leaf() {
        let region = ErrorRegion {
            span: span(2, 5),
            text: "@@@".into(),
        };
        assert_eq!(region.kind(), "ERROR");
        assert_eq!(region.text(), Some("@@@"));
        assert_eq!(region.span().byte_range, 2..5);
        assert!(region.is_named());
        assert!(region.children().is_empty());
    }

    /// `Visit` is `Copy`, so a consumer can keep a visit while it goes on.
    #[test]
    fn a_visit_is_copy() {
        let root = small_tree();
        let first = walk(root).next().expect("the root");
        let again = first;
        assert_eq!(first.depth, again.depth);
        assert_eq!(first.node.kind(), again.node.kind());
    }
}
