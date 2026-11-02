//! Insert-only AVL Euler sequence, following pinned Swift EulerWorkIndex.
//! Work lives in the block table; OPEN reads it, CLOSE contributes zero.
//! BigUint keeps all sums exact (including beyond Swift's 320-bit fast path).
use num_bigint::BigUint;
use std::collections::BTreeSet;
const NONE: usize = usize::MAX;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    Open(usize),
    Close(usize),
}
impl Event {
    fn node(self) -> usize {
        match self {
            Self::Open(h) => h.checked_mul(2).expect("Euler handle overflow"),
            Self::Close(h) => h
                .checked_mul(2)
                .and_then(|v| v.checked_add(1))
                .expect("Euler handle overflow"),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    sum: BigUint,
    left: usize,
    right: usize,
    parent: usize,
    height: u32,
}
impl Default for Node {
    fn default() -> Self {
        Self {
            sum: BigUint::default(),
            left: NONE,
            right: NONE,
            parent: NONE,
            height: 0,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EulerWorkIndex {
    nodes: Vec<Node>,
    root: usize,
}
impl Default for EulerWorkIndex {
    fn default() -> Self {
        Self {
            nodes: vec![],
            root: NONE,
        }
    }
}
impl EulerWorkIndex {
    pub(crate) fn contains(&self, h: usize) -> bool {
        h.checked_mul(2)
            .and_then(|n| self.nodes.get(n))
            .is_some_and(|n| n.height != 0)
    }
    fn height(&self, n: usize) -> u32 {
        if n == NONE { 0 } else { self.nodes[n].height }
    }
    fn reserve(&mut self, h: usize) {
        let len = h
            .checked_mul(2)
            .and_then(|n| n.checked_add(2))
            .expect("Euler handle overflow");
        if self.nodes.len() < len {
            self.nodes.resize_with(len, Node::default);
        }
    }
    fn value(n: usize, work: &[BigUint]) -> BigUint {
        if n & 1 == 0 {
            work[n / 2].clone()
        } else {
            BigUint::default()
        }
    }
    fn refresh(&mut self, n: usize, work: &[BigUint]) {
        let (l, r) = (self.nodes[n].left, self.nodes[n].right);
        self.nodes[n].height = 1 + self.height(l).max(self.height(r));
        let mut sum = Self::value(n, work);
        if l != NONE {
            sum += &self.nodes[l].sum;
        }
        if r != NONE {
            sum += &self.nodes[r].sum;
        }
        self.nodes[n].sum = sum;
    }
    fn rightmost(&self, mut n: usize) -> usize {
        while self.nodes[n].right != NONE {
            n = self.nodes[n].right;
        }
        n
    }
    fn replace_child(&mut self, parent: usize, old: usize, new: usize) {
        if parent == NONE {
            self.root = new;
        } else if self.nodes[parent].left == old {
            self.nodes[parent].left = new;
        } else {
            self.nodes[parent].right = new;
        }
        if new != NONE {
            self.nodes[new].parent = parent;
        }
    }
    fn rotate_left(&mut self, n: usize, work: &[BigUint]) {
        let p = self.nodes[n].right;
        if p == NONE {
            return;
        }
        let parent = self.nodes[n].parent;
        let middle = self.nodes[p].left;
        self.nodes[n].right = middle;
        if middle != NONE {
            self.nodes[middle].parent = n;
        }
        self.nodes[p].left = n;
        self.nodes[n].parent = p;
        self.replace_child(parent, n, p);
        self.refresh(n, work);
        self.refresh(p, work);
    }
    fn rotate_right(&mut self, n: usize, work: &[BigUint]) {
        let p = self.nodes[n].left;
        if p == NONE {
            return;
        }
        let parent = self.nodes[n].parent;
        let middle = self.nodes[p].right;
        self.nodes[n].left = middle;
        if middle != NONE {
            self.nodes[middle].parent = n;
        }
        self.nodes[p].right = n;
        self.nodes[n].parent = p;
        self.replace_child(parent, n, p);
        self.refresh(n, work);
        self.refresh(p, work);
    }
    fn rebalance(&mut self, mut n: usize, work: &[BigUint]) -> usize {
        let mut touched = 0;
        while n != NONE {
            self.refresh(n, work);
            touched += 1;
            let parent = self.nodes[n].parent;
            let (l, r) = (self.nodes[n].left, self.nodes[n].right);
            let balance = i64::from(self.height(l)) - i64::from(self.height(r));
            if balance > 1 {
                if self.height(self.nodes[l].left) < self.height(self.nodes[l].right) {
                    self.rotate_left(l, work);
                }
                self.rotate_right(n, work);
            } else if balance < -1 {
                if self.height(self.nodes[r].right) < self.height(self.nodes[r].left) {
                    self.rotate_right(r, work);
                }
                self.rotate_left(n, work);
            }
            n = parent;
        }
        touched
    }
    fn insert_before(&mut self, n: usize, anchor: usize, work: &[BigUint]) -> usize {
        self.nodes[n].height = 1;
        let host = if self.nodes[anchor].left == NONE {
            self.nodes[anchor].left = n;
            anchor
        } else {
            let host = self.rightmost(self.nodes[anchor].left);
            self.nodes[host].right = n;
            host
        };
        self.nodes[n].parent = host;
        self.refresh(n, work);
        self.rebalance(host, work)
    }
    fn append(&mut self, n: usize, work: &[BigUint]) {
        self.nodes[n].height = 1;
        if self.root == NONE {
            self.root = n;
            self.refresh(n, work);
            return;
        }
        let host = self.rightmost(self.root);
        self.nodes[host].right = n;
        self.nodes[n].parent = host;
        self.refresh(n, work);
        self.rebalance(host, work);
    }
    pub(crate) fn insert_root(&mut self, h: usize, work: &[BigUint]) -> bool {
        if h >= work.len()
            || h.checked_mul(2).and_then(|n| n.checked_add(2)).is_none()
            || self.contains(h)
        {
            return false;
        }
        self.reserve(h);
        self.append(h * 2, work);
        self.append(h * 2 + 1, work);
        true
    }
    pub(crate) fn insert_leaf(
        &mut self,
        h: usize,
        parent: usize,
        work: &[BigUint],
    ) -> Option<usize> {
        self.splice(&[Event::Open(h), Event::Close(h)], parent, work)
    }
    /// Validate the entire forest before mutation: a malformed splice leaves
    /// sequence, aggregates, and allocated nodes unchanged.
    pub(crate) fn splice(
        &mut self,
        events: &[Event],
        parent: usize,
        work: &[BigUint],
    ) -> Option<usize> {
        if !self.contains(parent) {
            return None;
        }
        let mut stack = vec![];
        let mut opened = BTreeSet::new();
        for &event in events {
            match event {
                Event::Open(h) => {
                    if h >= work.len()
                        || h.checked_mul(2).and_then(|n| n.checked_add(2)).is_none()
                        || self.contains(h)
                        || !opened.insert(h)
                    {
                        return None;
                    }
                    stack.push(h);
                }
                Event::Close(h) => {
                    if h >= work.len()
                        || h.checked_mul(2).and_then(|n| n.checked_add(1)).is_none()
                        || self.nodes.get(event.node()).is_some_and(|n| n.height != 0)
                        || stack.pop() != Some(h)
                    {
                        return None;
                    }
                }
            }
        }
        if !stack.is_empty() {
            return None;
        }
        let mut touched = 0;
        for &event in events {
            if let Event::Open(h) = event {
                self.reserve(h);
            }
            touched += self.insert_before(event.node(), parent * 2 + 1, work);
        }
        Some(touched)
    }
    pub(crate) fn changed(&mut self, h: usize, work: &[BigUint]) -> Option<usize> {
        if !self.contains(h) {
            return None;
        }
        let mut n = h * 2;
        let mut touched = 0;
        while n != NONE {
            self.refresh(n, work);
            touched += 1;
            n = self.nodes[n].parent;
        }
        Some(touched)
    }
    pub(crate) fn build(events: &[Event], work: &[BigUint]) -> Self {
        let mut index = Self::default();
        for &e in events {
            if let Event::Open(h) = e {
                index.reserve(h);
            }
        }
        fn build(index: &mut EulerWorkIndex, events: &[Event], work: &[BigUint]) -> usize {
            if events.is_empty() {
                return NONE;
            }
            let mid = events.len() / 2;
            let n = events[mid].node();
            let left = build(index, &events[..mid], work);
            let right = build(index, &events[mid + 1..], work);
            index.nodes[n].left = left;
            index.nodes[n].right = right;
            if left != NONE {
                index.nodes[left].parent = n;
            }
            if right != NONE {
                index.nodes[right].parent = n;
            }
            index.refresh(n, work);
            n
        }
        index.root = build(&mut index, events, work);
        index
    }
    fn prefix(&self, n: usize, work: &[BigUint]) -> (BigUint, usize) {
        let mut sum = Self::value(n, work);
        let left = self.nodes[n].left;
        if left != NONE {
            sum += &self.nodes[left].sum;
        }
        let mut current = n;
        let mut parent = self.nodes[current].parent;
        let mut visits = 1;
        while parent != NONE {
            visits += 1;
            if self.nodes[parent].right == current {
                let left = self.nodes[parent].left;
                if left != NONE {
                    sum += &self.nodes[left].sum;
                }
                sum += Self::value(parent, work);
            }
            current = parent;
            parent = self.nodes[current].parent;
        }
        (sum, visits)
    }
    pub(crate) fn weight_visiting(&self, h: usize, work: &[BigUint]) -> Option<(BigUint, usize)> {
        if !self.contains(h) {
            return None;
        }
        let (through, a) = self.prefix(h * 2 + 1, work);
        let (before, b) = self.prefix(h * 2, work);
        if through < before {
            return None;
        }
        Some((through - before + &work[h], a + b))
    }
    pub(crate) fn weight(&self, h: usize, work: &[BigUint]) -> Option<BigUint> {
        self.weight_visiting(h, work).map(|(sum, _)| sum)
    }
    fn precedes(&self, a: usize, b: usize) -> bool {
        if a == b {
            return false;
        }
        let depth = |mut n: usize| {
            let mut d = 0;
            while self.nodes[n].parent != NONE {
                n = self.nodes[n].parent;
                d += 1;
            }
            d
        };
        let (mut x, mut y) = (a, b);
        let (mut ax, mut by) = (NONE, NONE);
        let (mut dx, mut dy) = (depth(x), depth(y));
        while dx > dy {
            ax = x;
            x = self.nodes[x].parent;
            dx -= 1;
        }
        while dy > dx {
            by = y;
            y = self.nodes[y].parent;
            dy -= 1;
        }
        while x != y {
            ax = x;
            x = self.nodes[x].parent;
            by = y;
            y = self.nodes[y].parent;
        }
        let side = |child: usize| {
            if child == NONE {
                0
            } else if self.nodes[x].left == child {
                -1
            } else {
                1
            }
        };
        side(ax) < side(by)
    }
    #[cfg(test)]
    pub(crate) fn encloses(&self, a: usize, d: usize) -> Option<bool> {
        if !self.contains(a) || !self.contains(d) {
            return None;
        }
        Some(!self.precedes(d * 2, a * 2) && !self.precedes(a * 2 + 1, d * 2))
    }
    pub(crate) fn any_encloses(&self, ancestors: &[usize], descendants: &[usize]) -> bool {
        let mut ranges: Vec<_> = ancestors
            .iter()
            .copied()
            .filter(|&a| self.contains(a))
            .collect();
        ranges.sort_by(|&a, &b| {
            if a == b {
                std::cmp::Ordering::Equal
            } else if self.precedes(a * 2, b * 2) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        });
        let mut outer = vec![];
        for a in ranges {
            if outer
                .last()
                .is_some_and(|&last| !self.precedes(last * 2 + 1, a * 2))
            {
                continue;
            }
            outer.push(a);
        }
        descendants
            .iter()
            .copied()
            .filter(|&d| self.contains(d))
            .any(|d| {
                let at = outer.partition_point(|&a| !self.precedes(d * 2, a * 2));
                at > 0 && !self.precedes(outer[at - 1] * 2 + 1, d * 2)
            })
    }
    #[cfg(test)]
    fn sequence(&self) -> Vec<Event> {
        let mut out = vec![];
        let mut stack = vec![];
        let mut n = self.root;
        while n != NONE || !stack.is_empty() {
            while n != NONE {
                stack.push(n);
                n = self.nodes[n].left;
            }
            n = stack.pop().unwrap();
            out.push(if n & 1 == 0 {
                Event::Open(n / 2)
            } else {
                Event::Close(n / 2)
            });
            n = self.nodes[n].right;
        }
        out
    }
    #[cfg(test)]
    fn valid(&self, work: &[BigUint]) -> bool {
        if self.root != NONE && self.nodes[self.root].parent != NONE {
            return false;
        }
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.height > 0)
            .all(|(i, n)| {
                let mut sum = Self::value(i, work);
                for child in [n.left, n.right] {
                    if child != NONE {
                        if self.nodes[child].parent != i {
                            return false;
                        }
                        sum += &self.nodes[child].sum;
                    }
                }
                n.sum == sum
                    && n.height == 1 + self.height(n.left).max(self.height(n.right))
                    && self.height(n.left).abs_diff(self.height(n.right)) <= 1
            })
    }
}
#[cfg(test)]
mod tests;
