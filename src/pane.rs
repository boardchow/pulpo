//! The right side of the screen: each session's tabs, each a tree of panes
//! split in two, like tmux's windows and panes. A pane is a terminal, or
//! the file pane: the viewer, or the editor in its place. The file pane
//! follows you to the tab on screen.

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Pane {
    Term(usize),
    File,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Left,
    Up,
    Right,
    Down,
}

impl Dir {
    fn beside(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }

    /// Right or down: the way a split's second half lies.
    fn forward(self) -> bool {
        matches!(self, Dir::Right | Dir::Down)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Node {
    Leaf(Pane),
    /// `a` then `b`, side by side or one above the other; `a` gets `ratio`
    /// percent of the room.
    Split { beside: bool, ratio: u16, a: Box<Node>, b: Box<Node> },
}

/// How far a border can go, in percent.
const MIN_RATIO: u16 = 10;
const MAX_RATIO: u16 = 90;

impl Node {
    pub fn panes(&self) -> Vec<Pane> {
        match self {
            Node::Leaf(pane) => vec![*pane],
            Node::Split { a, b, .. } => [a.panes(), b.panes()].concat(),
        }
    }

    pub fn contains(&self, pane: Pane) -> bool {
        match self {
            Node::Leaf(p) => *p == pane,
            Node::Split { a, b, .. } => a.contains(pane) || b.contains(pane),
        }
    }

    /// Put `new` next to `at`, after it: on its right, or under it.
    pub fn split(&mut self, at: Pane, new: Pane, beside: bool, ratio: u16) -> bool {
        match self {
            Node::Leaf(p) if *p == at => {
                *self = Node::Split { beside, ratio, a: Box::new(Node::Leaf(at)), b: Box::new(Node::Leaf(new)) };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.split(at, new, beside, ratio) || b.split(at, new, beside, ratio),
        }
    }

    /// Take `pane` out; the other half of its split takes the room. None
    /// when nothing is left.
    pub fn without(self, pane: Pane) -> Option<Node> {
        match self {
            Node::Leaf(p) if p == pane => None,
            leaf @ Node::Leaf(_) => Some(leaf),
            Node::Split { beside, ratio, a, b } => match (a.without(pane), b.without(pane)) {
                (Some(a), Some(b)) => Some(Node::Split { beside, ratio, a: Box::new(a), b: Box::new(b) }),
                (Some(rest), None) | (None, Some(rest)) => Some(rest),
                (None, None) => None,
            },
        }
    }

    pub fn swap(&mut self, x: Pane, y: Pane) {
        match self {
            Node::Leaf(p) if *p == x => *p = y,
            Node::Leaf(p) if *p == y => *p = x,
            Node::Leaf(_) => {}
            Node::Split { a, b, .. } => {
                a.swap(x, y);
                b.swap(x, y);
            }
        }
    }

    /// Where each pane goes in `area`.
    pub fn layout(&self, area: Rect) -> Vec<(Pane, Rect)> {
        match self {
            Node::Leaf(pane) => vec![(*pane, area)],
            Node::Split { beside, ratio, a, b } => {
                let (first, second) = cut(area, *beside, *ratio);
                [a.layout(first), b.layout(second)].concat()
            }
        }
    }

    /// Move the border nearest to `pane` across `dir`'s axis, `step`
    /// percent toward `dir`.
    pub fn resize(&mut self, pane: Pane, dir: Dir, step: u16) -> bool {
        let Node::Split { beside, ratio, a, b } = self else { return false };
        let inner = if a.contains(pane) {
            a
        } else if b.contains(pane) {
            b
        } else {
            return false;
        };
        if inner.resize(pane, dir, step) {
            return true;
        }
        if *beside != dir.beside() {
            return false;
        }
        *ratio = if dir.forward() { (*ratio + step).min(MAX_RATIO) } else { ratio.saturating_sub(step).max(MIN_RATIO) };
        true
    }

    /// For layout.json: terminals by their tag; the file pane and unknown
    /// terminals are left out.
    pub fn save(&self, tag: &impl Fn(usize) -> Option<String>) -> Option<Saved> {
        match self {
            Node::Leaf(Pane::Term(id)) => tag(*id).map(Saved::Term),
            Node::Leaf(Pane::File) => None,
            Node::Split { beside, ratio, a, b } => match (a.save(tag), b.save(tag)) {
                (Some(a), Some(b)) => Some(Saved::Split { beside: *beside, ratio: *ratio, a: Box::new(a), b: Box::new(b) }),
                (Some(rest), None) | (None, Some(rest)) => Some(rest),
                (None, None) => None,
            },
        }
    }
}

/// `area` cut in two, the first part `ratio` percent of it.
fn cut(area: Rect, beside: bool, ratio: u16) -> (Rect, Rect) {
    let size = if beside { area.width } else { area.height };
    let first = ((size as u32 * ratio as u32 / 100) as u16).clamp(size.min(1), size.saturating_sub(1).max(size.min(1)));
    if beside {
        (Rect { width: first, ..area }, Rect { x: area.x + first, width: area.width - first, ..area })
    } else {
        (Rect { height: first, ..area }, Rect { y: area.y + first, height: area.height - first, ..area })
    }
}

/// The pane next to `from` toward `dir`: the nearest one that lines up
/// with it, the most where several do.
pub fn neighbor(panes: &[(Pane, Rect)], from: Pane, dir: Dir) -> Option<Pane> {
    let &(_, r) = panes.iter().find(|(p, _)| *p == from)?;
    let overlap = |a0: u16, a1: u16, b0: u16, b1: u16| a1.min(b1) as i32 - a0.max(b0) as i32;
    panes
        .iter()
        .filter(|(p, _)| *p != from)
        .filter_map(|&(p, o)| {
            let (gap, shared) = match dir {
                Dir::Left => (r.x as i32 - o.right() as i32, overlap(r.y, r.bottom(), o.y, o.bottom())),
                Dir::Right => (o.x as i32 - r.right() as i32, overlap(r.y, r.bottom(), o.y, o.bottom())),
                Dir::Up => (r.y as i32 - o.bottom() as i32, overlap(r.x, r.right(), o.x, o.right())),
                Dir::Down => (o.y as i32 - r.bottom() as i32, overlap(r.x, r.right(), o.x, o.right())),
            };
            (gap >= 0 && shared > 0).then_some(((gap, -shared), p))
        })
        .min_by_key(|&(key, _)| key)
        .map(|(_, p)| p)
}

/// A tab: its panes, the one you were last in, and the terminal you were
/// last in (where keys go when you come back to the tab's terminals).
#[derive(Clone, Debug)]
pub struct Tab {
    pub root: Node,
    pub focus: Pane,
    pub term: Option<usize>,
    pub name: Option<String>,
}

impl Tab {
    pub fn new(pane: Pane) -> Tab {
        let term = match pane {
            Pane::Term(id) => Some(id),
            Pane::File => None,
        };
        Tab { root: Node::Leaf(pane), focus: pane, term, name: None }
    }

    pub fn set_focus(&mut self, pane: Pane) {
        self.focus = pane;
        if let Pane::Term(id) = pane {
            self.term = Some(id);
        }
    }

    pub fn terms(&self) -> Vec<usize> {
        self.root
            .panes()
            .into_iter()
            .filter_map(|p| match p {
                Pane::Term(id) => Some(id),
                Pane::File => None,
            })
            .collect()
    }

    /// Take `pane` out; false when the tab is left empty.
    pub fn remove(&mut self, pane: Pane) -> bool {
        let root = std::mem::replace(&mut self.root, Node::Leaf(Pane::File));
        match root.without(pane) {
            Some(rest) => {
                self.root = rest;
                self.fix();
                true
            }
            None => false,
        }
    }

    /// Focus and terminal on panes that are still there.
    pub fn fix(&mut self) {
        let terms = self.terms();
        if self.term.is_none_or(|id| !terms.contains(&id)) {
            self.term = terms.first().copied();
        }
        if !self.root.contains(self.focus) {
            self.focus = self.term.map_or(Pane::File, Pane::Term);
        }
    }
}

/// A tab as layout.json keeps it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct SavedTab {
    pub root: Saved,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Saved {
    Term(String),
    Split { beside: bool, ratio: u16, a: Box<Saved>, b: Box<Saved> },
}

impl Saved {
    /// Back to panes, with the terminals that are still there.
    pub fn load(&self, id: &impl Fn(&str) -> Option<usize>) -> Option<Node> {
        match self {
            Saved::Term(tag) => id(tag).map(|id| Node::Leaf(Pane::Term(id))),
            Saved::Split { beside, ratio, a, b } => match (a.load(id), b.load(id)) {
                (Some(a), Some(b)) => Some(Node::Split { beside: *beside, ratio: (*ratio).clamp(MIN_RATIO, MAX_RATIO), a: Box::new(a), b: Box::new(b) }),
                (Some(rest), None) | (None, Some(rest)) => Some(rest),
                (None, None) => None,
            },
        }
    }

    /// A terminal brought back after a restart takes the old one's place.
    pub fn rename(&mut self, old: &str, new: &str) {
        match self {
            Saved::Term(tag) if tag == old => *tag = new.to_string(),
            Saved::Term(_) => {}
            Saved::Split { a, b, .. } => {
                a.rename(old, new);
                b.rename(old, new);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T1: Pane = Pane::Term(1);
    const T2: Pane = Pane::Term(2);
    const T3: Pane = Pane::Term(3);

    /// 1 on the left; 2 above 3 on the right.
    fn three() -> Node {
        let mut root = Node::Leaf(T1);
        assert!(root.split(T1, T2, true, 50));
        assert!(root.split(T2, T3, false, 50));
        root
    }

    #[test]
    fn splits_and_lays_out() {
        let root = three();
        assert_eq!(root.panes(), vec![T1, T2, T3]);
        let rects = root.layout(Rect::new(0, 0, 100, 40));
        assert_eq!(rects[0], (T1, Rect::new(0, 0, 50, 40)));
        assert_eq!(rects[1], (T2, Rect::new(50, 0, 50, 20)));
        assert_eq!(rects[2], (T3, Rect::new(50, 20, 50, 20)));
        // Even a tiny area gives each pane a cell.
        assert!(root.layout(Rect::new(0, 0, 2, 2)).iter().all(|(_, r)| r.width > 0 && r.height > 0));
    }

    #[test]
    fn finds_neighbors() {
        let rects = three().layout(Rect::new(0, 0, 100, 40));
        assert_eq!(neighbor(&rects, T1, Dir::Right), Some(T2));
        assert_eq!(neighbor(&rects, T3, Dir::Left), Some(T1));
        assert_eq!(neighbor(&rects, T2, Dir::Down), Some(T3));
        assert_eq!(neighbor(&rects, T3, Dir::Up), Some(T2));
        assert_eq!(neighbor(&rects, T1, Dir::Left), None);
        assert_eq!(neighbor(&rects, T1, Dir::Up), None);
    }

    #[test]
    fn removes_and_collapses() {
        let mut tab = Tab { root: three(), focus: T2, term: Some(2), name: None };
        assert!(tab.remove(T2));
        assert_eq!(tab.root.panes(), vec![T1, T3]);
        // Its focus moved to a pane still there.
        assert_eq!((tab.focus, tab.term), (T1, Some(1)));
        assert!(tab.remove(T1));
        assert_eq!(tab.root, Node::Leaf(T3));
        assert!(!tab.remove(T3));
    }

    #[test]
    fn resizes_the_nearest_border_on_the_axis() {
        let mut root = three();
        // Up from 3: the border between 2 and 3 moves up.
        assert!(root.resize(T3, Dir::Up, 10));
        // Right from 3: the border between 1 and the right half moves right.
        assert!(root.resize(T3, Dir::Right, 10));
        let Node::Split { ratio, b, .. } = &root else { panic!() };
        assert_eq!(*ratio, 60);
        let Node::Split { ratio, .. } = b.as_ref() else { panic!() };
        assert_eq!(*ratio, 40);
        // Never past 10% or 90%.
        for _ in 0..20 {
            root.resize(T1, Dir::Left, 10);
        }
        let Node::Split { ratio, .. } = &root else { panic!() };
        assert_eq!(*ratio, MIN_RATIO);
    }

    #[test]
    fn swaps_panes() {
        let mut root = three();
        root.swap(T1, T3);
        assert_eq!(root.panes(), vec![T3, T2, T1]);
    }

    #[test]
    fn saves_and_loads_by_tag() {
        let mut root = three();
        root.split(T3, Pane::File, false, 50);
        let tag = |id: usize| (id != 2).then(|| format!("t{id}"));
        // The file pane and the terminal without a tag are left out.
        let mut saved = root.save(&tag).unwrap();
        assert_eq!(
            saved,
            Saved::Split { beside: true, ratio: 50, a: Box::new(Saved::Term("t1".into())), b: Box::new(Saved::Term("t3".into())) }
        );
        saved.rename("t3", "t9");
        let id = |tag: &str| tag.strip_prefix('t').and_then(|n| n.parse().ok());
        let loaded = saved.load(&id).unwrap();
        assert_eq!(loaded.panes(), vec![T1, Pane::Term(9)]);
        // A terminal that is gone leaves its half to the other.
        let only = |tag: &str| (tag == "t1").then_some(1);
        assert_eq!(saved.load(&only), Some(Node::Leaf(T1)));
    }
}
