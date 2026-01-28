use std::{collections::VecDeque, fmt::Debug};

use serde::{Deserialize, Serialize};

pub mod demangle;
pub mod entrypoint;
pub mod invoke;
pub mod loc;
pub mod view;

pub trait TreeNode: Clone + PartialEq {
    fn root() -> Self;

    fn index(index: usize) -> Self;

    fn can_push_to(&self, tree: &Tree<Self>) -> bool;

    fn is_leaf(&self) -> bool;

    fn is_fn_call(&self) -> bool;
}

fn is_dep_fn_call<N: TreeNode>(tree: &Tree<N>) -> bool {
    tree.node.is_fn_call() && tree.children.len() == 0
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tree<N: TreeNode> {
    instruction: Option<u64>,
    node: N,
    children: Vec<Tree<N>>,
}

impl<N: TreeNode> Tree<N> {
    pub fn new() -> Self {
        Self {
            instruction: None,
            node: N::root(),
            children: vec![],
        }
    }

    pub fn push_branch(&mut self, i: u64, nodes: VecDeque<N>) {
        self.grow(i, &nodes, 0);
    }

    pub fn push_leaf(&mut self, node: N, cursor: Option<&N>) {
        let subtree = self.get_pushable_subtree(&node, cursor);
        let instruction = subtree.instruction;
        subtree.children.push(Tree {
            instruction,
            node,
            children: vec![],
        });
    }

    fn grow(&mut self, i: u64, nodes: &VecDeque<N>, counter: usize) {
        let Some(node) = nodes.get(counter) else {
            return;
        };

        if self.children.last().map_or(true, |last| &last.node != node) {
            self.children.push(Tree {
                instruction: Some(i),
                node: node.clone(),
                children: vec![],
            });
        }

        self.children
            .last_mut()
            .unwrap()
            .grow(i, nodes, counter + 1);
    }

    fn get_pushable_subtree(&mut self, node: &N, cursor: Option<&N>) -> &mut Self {
        if cursor.map_or(false, |c| self.node == *c) {
            return self;
        }

        let should_recurse = self
            .children
            .last()
            .map(|last_child| node.can_push_to(last_child))
            .unwrap_or(false);

        if should_recurse {
            self.children
                .last_mut()
                .unwrap()
                .get_pushable_subtree(node, cursor)
        } else {
            self
        }
    }

    fn colone_last_branch(&self) -> Self {
        let mut new_tree = Self::new();

        let mut source_tree = self;
        let mut dest_tree = &mut new_tree;

        loop {
            if let Some(last_child) = source_tree.children.last() {
                dest_tree.children.push(Tree { 
                    instruction: last_child.instruction, 
                    node: last_child.node.clone(), 
                    children: vec![],
                });
                dest_tree = dest_tree.children.last_mut().unwrap();
                source_tree = source_tree.children.last().unwrap();
            } else {
                break;
            }
        }

        new_tree
    }

    fn get_absolute_last_child(&mut self) -> &mut Self {
        if self.children.last().is_some() {
            self.children.last_mut().unwrap().get_absolute_last_child()
        } else {
            self
        }
    }
}

#[derive(Debug)]
pub struct UniqueTree<I: PartialEq, N: TreeNode> {
    pub id: I,
    pub tree: Tree<N>,
}

#[derive(Debug)]
pub struct TemporalTrace<A> {
    subtree_index: usize,
    additional_data: A,
}

#[derive(Debug)]
pub struct TreeContext<I: PartialEq, N: TreeNode, A = ()> {
    trace: Vec<TemporalTrace<A>>,
    subtrees: Vec<UniqueTree<I, N>>,
}

impl<I: PartialEq + Debug, N: TreeNode, A> TreeContext<I, N, A> {
    pub fn push_subtree(&mut self, id: I, additional_data: A) {
        let subtree_index = self.subtrees.len();

        if let Some(last_trace) = self.trace.last_mut() {
            self.subtrees[last_trace.subtree_index]
                .tree
                .push_leaf(N::index(subtree_index), None);
        }

        self.subtrees.push(UniqueTree {
            id,
            tree: Tree::new(),
        });
        self.trace.push(TemporalTrace {
            subtree_index,
            additional_data,
        });
    }

    pub fn get_current_unique_tree(&mut self) -> &mut UniqueTree<I, N> {
        self.subtrees
            .get_mut(
                self.trace
                    .last()
                    .expect("Last trace must exist when calling CUT")
                    .subtree_index,
            )
            .expect("CUT must exist by the time get_current_unique_tree is called")
    }
}
