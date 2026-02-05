use serde::{Deserialize, Serialize};

pub struct LookupNode<T> {
    center: u64,
    overlaps: Vec<LookupInterval<T>>,
    left: Option<Box<LookupNode<T>>>,
    right: Option<Box<LookupNode<T>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LookupInterval<T> {
    pub begin: u64,
    pub end: u64,
    pub depth: u32,
    pub data: T,
}

impl<T> LookupNode<T> {
    pub fn build(intervals: Vec<LookupInterval<T>>) -> Option<Box<Self>> {
        if intervals.is_empty() {
            return None;
        }

        let mut points: Vec<u64> = intervals.iter().map(|i| (i.begin + i.end) / 2).collect();
        points.sort();
        let center = points[points.len() / 2];

        let mut overlaps = Vec::new();
        let mut left = Vec::new();
        let mut right = Vec::new();

        for i in intervals {
            if i.begin <= center && center < i.end {
                overlaps.push(i);
            } else if center < i.begin {
                right.push(i);
            } else {
                left.push(i);
            }
        }

        Some(Box::new(LookupNode {
            center: center,
            overlaps: overlaps,
            left: LookupNode::<T>::build(left),
            right: LookupNode::<T>::build(right),
        }))
    }
}

impl<T: Clone> LookupNode<T> {
    pub fn search_deepest<'a>(&'a self, pc: &u64) -> Option<&'a LookupInterval<T>> {
        let pc_lookup = *pc;
        let mut best: Option<&LookupInterval<T>> = None;

        for iv in &self.overlaps {
            if iv.begin <= pc_lookup && pc_lookup < iv.end {
                if best.map_or(true, |b| iv.depth > b.depth) {
                    best = Some(iv);
                }
            }
        }

        if pc_lookup < self.center {
            if let Some(left) = &self.left {
                if let Some(candidate) = left.search_deepest(pc) {
                    if best.map_or(true, |b| candidate.depth > b.depth) {
                        best = Some(candidate);
                    }
                }
            }
        } else if pc_lookup > self.center {
            if let Some(right) = &self.right {
                if let Some(candidate) = right.search_deepest(pc) {
                    if best.map_or(true, |b| candidate.depth > b.depth) {
                        best = Some(candidate);
                    }
                }
            }
        }

        best
    }
}
