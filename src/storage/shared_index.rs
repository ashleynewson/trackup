use std::cell::RefCell;
use std::rc::Rc;

use super::Index;

struct SharedIndexInternal {
    pub layer_count: usize,
    pub hole_count: usize,
    pub chunk_layers: Vec<usize>,
    pub chunk_offsets: Vec<u64>,
}

/// A shared index which points to both the highest priority store and
/// the offset in that store for a given chunk number.
///
/// Used for accessing the logical data of an incremental snapshot,
/// which may consist of multiple layers of stores.
///
/// Layers are added from the top down. The top layer is layer 0 and
/// has the highest priority.
pub struct SharedIndex(Rc<RefCell<SharedIndexInternal>>);

impl SharedIndex {
    pub fn new(chunk_count: usize) -> Self {
        Self(Rc::new(RefCell::new(SharedIndexInternal {
            layer_count: 0,
            hole_count: chunk_count,
            chunk_layers: vec![std::usize::MAX; chunk_count],
            chunk_offsets: vec![std::u64::MAX; chunk_count],
        })))
    }
    pub fn add_layer(&self, chunk_count: usize) -> SharedIndexHandle {
        let mut internal = self.0.borrow_mut();
        if chunk_count != internal.chunk_offsets.len() {
            panic!("Chunk count mismatch between index layers");
        }
        let handle = SharedIndexHandle::new(&self.0, internal.layer_count);
        internal.layer_count = internal.layer_count + 1;
        handle
    }
    pub fn lookup_layer(&self, chunk_number: usize) -> Option<usize> {
        let internal = self.0.borrow();
        match internal.chunk_layers[chunk_number] {
            std::usize::MAX => {
                None
            },
            layer => {
                Some(layer)
            }
        }
    }
    pub fn is_complete(&self) -> bool {
        self.0.borrow().hole_count == 0
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
    pub fn layer_number(&self) -> usize {
        self.layer
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
        if internal.chunk_layers[chunk_number] >= self.layer {
            if internal.chunk_layers[chunk_number] == std::usize::MAX {
                internal.hole_count = internal.hole_count - 1;
            }
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
            if self.layer != 0 {
                panic!("Index lookup miss for non-top layers are not expected");
            }
            None
        }
    }
}
