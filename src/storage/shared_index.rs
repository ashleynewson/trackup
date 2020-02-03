use std::cell::RefCell;
use std::rc::Rc;

use super::Index;

struct SharedIndexInternal {
    pub top_layer: usize,
    pub chunk_layers: Vec<usize>,
    pub chunk_offsets: Vec<u64>,
}

/// A shared index which points to both the highest priority store and
/// the offset in that store for a given chunk number.
///
/// Used for accessing the logical data of an incremental snapshot,
/// which may consist of multiple layers of stores.
//
// Note that internally, layer 0 means no layer. However, the external
// interface presents the first layer as layer 0.
pub struct SharedIndex(Rc<RefCell<SharedIndexInternal>>);

impl SharedIndex {
    pub fn new(chunk_count: usize) -> Self {
        Self(Rc::new(RefCell::new(SharedIndexInternal {
            top_layer: 0,
            chunk_layers: vec![0; chunk_count],
            chunk_offsets: vec![std::u64::MAX; chunk_count],
        })))
    }
    pub fn add_layer(&self, chunk_count: usize) -> SharedIndexHandle {
        let mut internal = self.0.borrow_mut();
        if chunk_count != internal.chunk_offsets.len() {
            panic!("Chunk count mismatch between index layers");
        }
        internal.top_layer = internal.top_layer + 1;
        let handle = SharedIndexHandle::new(&self.0, internal.top_layer);
        handle
    }
    pub fn lookup(&self, chunk_number: usize) -> Option<(usize, u64)> {
        let internal = self.0.borrow();
        match internal.chunk_layers[chunk_number] {
            0 => {
                None
            },
            layer => {
                Some((layer-1, internal.chunk_offsets[chunk_number]))
            }
        }
    }
}

pub struct SharedIndexHandle {
    shared_index: Rc<RefCell<SharedIndexInternal>>,
    layer: usize,
}

/// The view of a shared index from a particular store's perspective.
impl SharedIndexHandle {
    pub(self) fn new(shared_index: &Rc<RefCell<SharedIndexInternal>>, layer: usize) -> Self {
        Self {
            shared_index: Rc::clone(shared_index),
            layer,
        }
    }
    // Note, we use layer 0 internally to mean no layer. However,
    // externally we want the interface to be zero-indexed.
    pub fn layer_number(&self) -> usize {
        self.layer - 1
    }
}

impl Index for SharedIndexHandle {
    /// Record an chunk's offset for this layer into the index.
    ///
    /// Note that this will only update the chunk's offset and layer if
    /// it is equal to or above the existing layer for that chunk.
    fn replace(&mut self, chunk_number: usize, offset: u64) {
        if offset == std::u64::MAX {
            panic!("0xffff_ffff_ffff_ffff is a reserved value not permitted in indexes");
        }
        let mut internal = self.shared_index.borrow_mut();
        if internal.chunk_layers[chunk_number] <= self.layer {
            internal.chunk_layers[chunk_number] = self.layer;
            internal.chunk_offsets[chunk_number] = offset;
        }
    }
    /// Perform a lookup in the index for owned offsets.
    ///
    /// Note that offsets from other chunks are never returned.
    ///
    /// Handles for the top layer may query any chunk, returning None
    /// for chunks not in the top layer.
    ///
    /// Handles for other layers are only permitted to look up chunks
    /// which they contain. (They should never be querying them
    /// otherwise.)
    fn lookup(&self, chunk_number: usize) -> Option<u64> {
        let internal = self.shared_index.borrow();
        if internal.chunk_layers[chunk_number] == self.layer {
            Some(internal.chunk_offsets[chunk_number])
        } else {
            if self.layer != internal.top_layer {
                panic!("Index lookup miss for non-top layers are not expected");
            }
            None
        }
    }
}
