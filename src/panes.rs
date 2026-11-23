//! A workspace's panes: a tree of splits, each leaf a terminal.

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Node<T> {
    Leaf(T),
    /// `down`: `a` above `b`, else `a` left of `b`.
    Split { down: bool, a: Box<Node<T>>, b: Box<Node<T>> },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl<T: Clone + PartialEq> Node<T> {
    pub fn leaves(&self) -> Vec<T> {
        match self {
            Node::Leaf(t) => vec![t.clone()],
            Node::Split { a, b, .. } => [a.leaves(), b.leaves()].concat(),
        }
    }

    pub fn contains(&self, leaf: &T) -> bool {
        match self {
            Node::Leaf(t) => t == leaf,
            Node::Split { a, b, .. } => a.contains(leaf) || b.contains(leaf),
        }
    }

    /// Whether `at` was there.
    pub fn split(&mut self, at: &T, new: T, down: bool) -> bool {
        match self {
            Node::Leaf(t) if t == at => {
                *self = Node::Split { down, a: Box::new(Node::Leaf(t.clone())), b: Box::new(Node::Leaf(new)) };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.split(at, new.clone(), down) || b.split(at, new, down),
        }
    }

    pub fn replace(&mut self, old: &T, new: T) {
        match self {
            Node::Leaf(t) if t == old => *t = new,
            Node::Leaf(_) => {}
            Node::Split { a, b, .. } => {
                a.replace(old, new.clone());
                b.replace(old, new);
            }
        }
    }

    /// The leaves `keep` gives a value for, the splits left with one side gone.
    pub fn filter_map<U>(&self, keep: &impl Fn(&T) -> Option<U>) -> Option<Node<U>> {
        match self {
            Node::Leaf(t) => keep(t).map(Node::Leaf),
            Node::Split { down, a, b } => match (a.filter_map(keep), b.filter_map(keep)) {
                (Some(a), Some(b)) => Some(Node::Split { down: *down, a: Box::new(a), b: Box::new(b) }),
                (one, other) => one.or(other),
            },
        }
    }

    pub fn rects(&self, area: Rect) -> Vec<(T, Rect)> {
        match self {
            Node::Leaf(t) => vec![(t.clone(), area)],
            Node::Split { down, a, b } => {
                let (first, second) = if *down {
                    let h = area.height / 2;
                    (Rect { height: h, ..area }, Rect { y: area.y + h, height: area.height - h, ..area })
                } else {
                    let w = area.width / 2;
                    (Rect { width: w, ..area }, Rect { x: area.x + w, width: area.width - w, ..area })
                };
                [a.rects(first), b.rects(second)].concat()
            }
        }
    }
}

/// The pane next to `from` in that direction, the nearest one that shares some of its rows or columns.
pub fn neighbor<T: PartialEq + Clone>(rects: &[(T, Rect)], from: &T, dir: Dir) -> Option<T> {
    let cur = rects.iter().find(|(t, _)| t == from)?.1;
    let overlap = |a: (u16, u16), b: (u16, u16)| a.0 < b.1 && b.0 < a.1;
    let (cx, cy) = (cur.x + cur.width / 2, cur.y + cur.height / 2);
    rects
        .iter()
        .filter(|(t, _)| t != from)
        .filter_map(|(t, r)| {
            let (gap, shares, along) = match dir {
                Dir::Left => (cur.x.checked_sub(r.right())?, overlap((cur.y, cur.bottom()), (r.y, r.bottom())), r.y + r.height / 2),
                Dir::Right => (r.x.checked_sub(cur.right())?, overlap((cur.y, cur.bottom()), (r.y, r.bottom())), r.y + r.height / 2),
                Dir::Up => (cur.y.checked_sub(r.bottom())?, overlap((cur.x, cur.right()), (r.x, r.right())), r.x + r.width / 2),
                Dir::Down => (r.y.checked_sub(cur.bottom())?, overlap((cur.x, cur.right()), (r.x, r.right())), r.x + r.width / 2),
            };
            let across = if matches!(dir, Dir::Left | Dir::Right) { cy } else { cx };
            shares.then(|| ((gap, along.abs_diff(across)), t.clone()))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, t)| t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Node<u32> {
        // 1 | (2 over 3)
        let mut n = Node::Leaf(1);
        n.split(&1, 2, false);
        n.split(&2, 3, true);
        n
    }

    #[test]
    fn splits_in_place_and_lists_leaves_in_order() {
        assert_eq!(tree().leaves(), vec![1, 2, 3]);
        assert!(!tree().split(&9, 4, true));
    }

    #[test]
    fn dropping_a_leaf_gives_its_room_to_the_sibling() {
        let t = tree().filter_map(&|t| (*t != 2).then_some(*t)).unwrap();
        let rects = t.rects(Rect::new(0, 0, 80, 24));
        assert_eq!(rects, vec![(1, Rect::new(0, 0, 40, 24)), (3, Rect::new(40, 0, 40, 24))]);
        assert!(tree().filter_map(&|_| None::<u32>).is_none());
    }

    #[test]
    fn finds_the_neighbor_in_each_direction() {
        let rects = tree().rects(Rect::new(0, 0, 80, 24));
        assert_eq!(neighbor(&rects, &1, Dir::Right), Some(2));
        assert_eq!(neighbor(&rects, &3, Dir::Left), Some(1));
        assert_eq!(neighbor(&rects, &2, Dir::Down), Some(3));
        assert_eq!(neighbor(&rects, &3, Dir::Up), Some(2));
        assert_eq!(neighbor(&rects, &1, Dir::Left), None);
        assert_eq!(neighbor(&rects, &3, Dir::Down), None);
    }
}
