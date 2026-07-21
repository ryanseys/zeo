//! A constant name as it was WRITTEN -- `Foo`, `A::B::C`, or a top-level-
//! anchored `::Foo` -- with its pieces kept apart.
//!
//! `parse::constant_path_name` produces the joined spelling as one `String`,
//! and that is what `HirNode`'s name fields carry. The hazard is that a joined
//! name LOOKS like a flat one, so a consumer reasonably treats it as a flat
//! key and nothing fails until a namespaced class appears. That bug shipped
//! four separate times in one day -- a `ConstWrite` that stored a constant
//! literally spelled `"NS::Item"` under `Object` (leaving `NS.constants`
//! empty), a `.name` that answered the bare leaf instead of the qualified
//! path, and two lookups that searched `Object` for the joined spelling and
//! raised `NameError`.
//!
//! Every place that needs the pieces goes through this type, so the splitting
//! rules -- including the `::` anchor, which is easy to mishandle into an
//! EMPTY scope -- are written and tested once.

/// A parsed constant path. Borrows the joined spelling it was built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstPath<'a> {
    /// The spelling as written, `::` anchor included.
    joined: &'a str,
    /// `joined` minus any leading `::`.
    body: &'a str,
}

impl<'a> ConstPath<'a> {
    pub fn parse(joined: &'a str) -> ConstPath<'a> {
        ConstPath { joined, body: joined.strip_prefix("::").unwrap_or(joined) }
    }

    /// The spelling as written -- what `HirNode`'s name fields carry.
    pub fn joined(&self) -> &'a str {
        self.joined
    }

    /// The LAST segment: the constant actually being named or defined.
    pub fn base(&self) -> &'a str {
        match self.body.rsplit_once("::") {
            Some((_, base)) => base,
            None => self.body,
        }
    }

    /// Everything before the last `::`, or `None` for a single segment.
    ///
    /// Keeps the `::` anchor on the prefix (`::A::B` scopes to `::A`) so the
    /// result stays resolvable by the same rules as the original. A lone
    /// `::Foo` has NO scope -- the anchor means "top level", not "a namespace
    /// spelled empty", and treating it as the latter is exactly the mistake a
    /// hand-rolled `rsplit_once` makes.
    pub fn scope(&self) -> Option<&'a str> {
        let (prefix, _) = self.body.rsplit_once("::")?;
        let start = self.joined.len() - self.body.len();
        Some(&self.joined[..start + prefix.len()])
    }

    /// Each segment in order, anchor excluded (`::A::B` yields `A`, `B`).
    /// Always at least one item.
    pub fn segments(&self) -> impl Iterator<Item = &'a str> {
        self.body.split("::")
    }

    /// A leading `::` -- resolution starts at the top level, skipping the
    /// lexical chain.
    pub fn is_top_anchored(&self) -> bool {
        self.joined.len() != self.body.len()
    }

    /// Neither namespaced nor anchored: an ordinary name resolved through the
    /// enclosing lexical scope.
    pub fn is_bare(&self) -> bool {
        self.scope().is_none() && !self.is_top_anchored()
    }
}

#[cfg(test)]
mod tests {
    use super::ConstPath;

    #[test]
    fn a_bare_name_has_no_scope() {
        let p = ConstPath::parse("Foo");
        assert_eq!(p.base(), "Foo");
        assert_eq!(p.scope(), None);
        assert!(!p.is_top_anchored());
        assert!(p.is_bare());
    }

    #[test]
    fn a_namespaced_name_splits_at_the_last_separator() {
        let p = ConstPath::parse("A::B::C");
        assert_eq!(p.base(), "C");
        assert_eq!(p.scope(), Some("A::B"));
        assert!(!p.is_top_anchored());
        assert!(!p.is_bare());
    }

    /// The anchor means "top level", NOT a namespace spelled empty -- a plain
    /// `rsplit_once("::")` yields `("", "Foo")` here, and a consumer that
    /// trusts it looks the constant up inside a scope that does not exist.
    #[test]
    fn a_top_anchored_single_segment_has_no_scope() {
        let p = ConstPath::parse("::Foo");
        assert_eq!(p.base(), "Foo");
        assert_eq!(p.scope(), None);
        assert!(p.is_top_anchored());
        assert!(!p.is_bare());
    }

    /// An anchored PATH keeps the anchor on its scope, so the prefix resolves
    /// by the same rules the whole path would.
    #[test]
    fn an_anchored_path_keeps_the_anchor_on_its_scope() {
        let p = ConstPath::parse("::A::B");
        assert_eq!(p.base(), "B");
        assert_eq!(p.scope(), Some("::A"));
        assert!(p.is_top_anchored());
    }

    #[test]
    fn joined_round_trips_the_original_spelling() {
        for s in ["Foo", "A::B", "::Foo", "::A::B::C"] {
            assert_eq!(ConstPath::parse(s).joined(), s);
        }
    }
}
