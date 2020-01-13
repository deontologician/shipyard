mod pack_info;
pub mod sort;
mod view_add_entity;
mod windows;

use crate::storage::EntityId;
pub(crate) use pack_info::{LoosePack, Pack, PackInfo, TightPack, UpdatePack};
use std::ptr;
pub(crate) use view_add_entity::ViewAddEntity;
//pub(crate) use windows::RawWindowMut;
pub use windows::{Window, WindowMut};

// A sparse array is a data structure with 2 vectors: one sparse, the other dense.
// Only usize can be added. On insertion, the number is pushed into the dense vector
// and sparse[number] is set to dense.len() - 1.
// For all number present in the sparse array, dense[sparse[number]] == number.
// For all other values if set sparse[number] will have any value left there
// and if set dense[sparse[number]] != number.
// We can't be limited to store solely integers, this is why there is a third vector.
// It mimics the dense vector in regard to insertion/deletion.
pub struct SparseSet<T> {
    pub(crate) sparse: Vec<usize>,
    pub(crate) dense: Vec<EntityId>,
    pub(crate) data: Vec<T>,
    pub(crate) pack_info: PackInfo<T>,
}

impl<T> Default for SparseSet<T> {
    fn default() -> Self {
        SparseSet {
            sparse: Vec::new(),
            dense: Vec::new(),
            data: Vec::new(),
            pack_info: Default::default(),
        }
    }
}

impl<T> SparseSet<T> {
    pub(crate) fn window(&self) -> Window<'_, T> {
        Window {
            sparse: &self.sparse,
            dense: &self.dense,
            data: &self.data,
            pack_info: &self.pack_info,
        }
    }
    pub(crate) fn window_mut(&mut self) -> WindowMut<'_, T> {
        WindowMut {
            sparse: &mut self.sparse,
            dense: &mut self.dense,
            data: &mut self.data,
            pack_info: &mut self.pack_info,
        }
    }
    pub(crate) fn insert(&mut self, mut value: T, entity: EntityId) -> Option<T> {
        if entity.index() >= self.sparse.len() {
            self.sparse.resize(entity.index() + 1, 0);
        }
        if let Some(data) = self.get_mut(entity) {
            std::mem::swap(data, &mut value);
            Some(value)
        } else {
            unsafe { *self.sparse.get_unchecked_mut(entity.index()) = self.dense.len() };
            self.dense.push(entity);
            self.data.push(value);
            None
        }
    }
    pub(crate) fn remove(&mut self, entity: EntityId) -> Option<T> {
        if self.contains(entity) {
            let mut dense_index = unsafe { *self.sparse.get_unchecked(entity.index()) };
            match &mut self.pack_info.pack {
                Pack::Tight(pack_info) => {
                    let pack_len = pack_info.len;
                    if dense_index < pack_len {
                        pack_info.len -= 1;
                        // swap index and last packed element (can be the same)
                        unsafe {
                            *self.sparse.get_unchecked_mut(
                                self.dense.get_unchecked(pack_len - 1).index(),
                            ) = dense_index;
                        }
                        self.dense.swap(dense_index, pack_len - 1);
                        self.data.swap(dense_index, pack_len - 1);
                        dense_index = pack_len - 1;
                    }
                }
                Pack::Loose(pack_info) => {
                    let pack_len = pack_info.len;
                    if dense_index < pack_len {
                        pack_info.len -= 1;
                        // swap index and last packed element (can be the same)
                        unsafe {
                            *self.sparse.get_unchecked_mut(
                                self.dense.get_unchecked(pack_len - 1).index(),
                            ) = dense_index;
                        }
                        self.dense.swap(dense_index, pack_len - 1);
                        self.data.swap(dense_index, pack_len - 1);
                        dense_index = pack_len - 1;
                    }
                }
                Pack::Update(pack) => {
                    if dense_index < pack.inserted {
                        pack.inserted -= 1;
                        unsafe {
                            *self.sparse.get_unchecked_mut(
                                self.dense.get_unchecked(pack.inserted).index(),
                            ) = dense_index;
                        }
                        self.dense.swap(dense_index, pack.inserted);
                        self.data.swap(dense_index, pack.inserted);
                        dense_index = pack.inserted;
                    }
                    if dense_index < pack.inserted + pack.modified {
                        pack.modified -= 1;
                        unsafe {
                            *self.sparse.get_unchecked_mut(
                                self.dense
                                    .get_unchecked(pack.inserted + pack.modified)
                                    .index(),
                            ) = dense_index;
                        }
                        self.dense.swap(dense_index, pack.inserted + pack.modified);
                        self.data.swap(dense_index, pack.inserted + pack.modified);
                        dense_index = pack.inserted + pack.modified;
                    }
                }
                Pack::NoPack => {}
            }
            unsafe {
                *self
                    .sparse
                    .get_unchecked_mut(self.dense.get_unchecked(self.dense.len() - 1).index()) =
                    dense_index;
            }
            self.dense.swap_remove(dense_index);
            Some(self.data.swap_remove(dense_index))
        } else {
            None
        }
    }
    pub fn contains(&self, entity: EntityId) -> bool {
        self.window().contains(entity)
    }
    pub(crate) fn get(&self, entity: EntityId) -> Option<&T> {
        if self.contains(entity) {
            Some(unsafe {
                self.data
                    .get_unchecked(*self.sparse.get_unchecked(entity.index()))
            })
        } else {
            None
        }
    }
    pub(crate) fn get_mut(&mut self, entity: EntityId) -> Option<&mut T> {
        if self.contains(entity) {
            // SAFE we checked the window countains the entity
            let mut index = unsafe { *self.sparse.get_unchecked(entity.index()) };
            if let Pack::Update(pack) = &mut self.pack_info.pack {
                if index >= pack.modified {
                    // index of the first element non modified
                    let non_mod = pack.inserted + pack.modified;
                    if index >= non_mod {
                        // SAFE we checked the window contains the entity
                        unsafe {
                            ptr::swap(
                                self.dense.get_unchecked_mut(non_mod),
                                self.dense.get_unchecked_mut(index),
                            );
                            ptr::swap(
                                self.data.get_unchecked_mut(non_mod),
                                self.data.get_unchecked_mut(index),
                            );
                            *self
                                .sparse
                                .get_unchecked_mut((*self.dense.get_unchecked(non_mod)).index()) =
                                non_mod;
                            *self
                                .sparse
                                .get_unchecked_mut((*self.dense.get_unchecked(index)).index()) =
                                index;
                        }
                        pack.modified += 1;
                        index = non_mod;
                    }
                }
            }
            Some(unsafe { self.data.get_unchecked_mut(index) })
        } else {
            None
        }
    }
    pub fn len(&self) -> usize {
        self.window().len()
    }
    pub fn is_empty(&self) -> bool {
        self.window().is_empty()
    }
    pub fn inserted(&self) -> Window<'_, T> {
        match &self.pack_info.pack {
            Pack::Update(pack) => Window {
                sparse: &self.sparse,
                dense: &self.dense[0..pack.inserted],
                data: &self.data[0..pack.inserted],
                pack_info: &self.pack_info,
            },
            _ => Window {
                sparse: &[],
                dense: &[],
                data: &[],
                pack_info: &self.pack_info,
            },
        }
    }
    pub fn inserted_mut(&mut self) -> WindowMut<'_, T> {
        match &self.pack_info.pack {
            Pack::Update(pack) => WindowMut {
                sparse: &mut self.sparse,
                dense: &mut self.dense[0..pack.inserted],
                data: &mut self.data[0..pack.inserted],
                pack_info: &mut self.pack_info,
            },
            _ => WindowMut {
                sparse: &mut [],
                dense: &mut [],
                data: &mut [],
                pack_info: &mut self.pack_info,
            },
        }
    }
    pub fn modified(&self) -> Window<'_, T> {
        match &self.pack_info.pack {
            Pack::Update(pack) => Window {
                sparse: &self.sparse,
                dense: &self.dense[pack.inserted..pack.inserted + pack.modified],
                data: &self.data[pack.inserted..pack.inserted + pack.modified],
                pack_info: &self.pack_info,
            },
            _ => Window {
                sparse: &[],
                dense: &[],
                data: &[],
                pack_info: &self.pack_info,
            },
        }
    }
    pub fn modified_mut(&mut self) -> WindowMut<'_, T> {
        match &self.pack_info.pack {
            Pack::Update(pack) => WindowMut {
                sparse: &mut self.sparse,
                dense: &mut self.dense[pack.inserted..pack.inserted + pack.modified],
                data: &mut self.data[pack.inserted..pack.inserted + pack.modified],
                pack_info: &mut self.pack_info,
            },
            _ => WindowMut {
                sparse: &mut [],
                dense: &mut [],
                data: &mut [],
                pack_info: &mut self.pack_info,
            },
        }
    }
    pub fn inserted_or_modified(&self) -> Window<'_, T> {
        match &self.pack_info.pack {
            Pack::Update(pack) => Window {
                sparse: &self.sparse,
                dense: &self.dense[0..pack.inserted + pack.modified],
                data: &self.data[0..pack.inserted + pack.modified],
                pack_info: &self.pack_info,
            },
            _ => Window {
                sparse: &[],
                dense: &[],
                data: &[],
                pack_info: &self.pack_info,
            },
        }
    }
    pub fn inserted_or_modified_mut(&mut self) -> WindowMut<'_, T> {
        match &self.pack_info.pack {
            Pack::Update(pack) => WindowMut {
                sparse: &mut self.sparse,
                dense: &mut self.dense[0..pack.inserted + pack.modified],
                data: &mut self.data[0..pack.inserted + pack.modified],
                pack_info: &mut self.pack_info,
            },
            _ => WindowMut {
                sparse: &mut [],
                dense: &mut [],
                data: &mut [],
                pack_info: &mut self.pack_info,
            },
        }
    }
    pub fn take_removed(&mut self) -> Option<Vec<(EntityId, T)>> {
        match &mut self.pack_info.pack {
            Pack::Update(pack) => {
                let mut vec = Vec::with_capacity(pack.removed.capacity());
                std::mem::swap(&mut vec, &mut pack.removed);
                Some(vec)
            }
            _ => None,
        }
    }
    pub fn clear_inserted(&mut self) {
        if let Pack::Update(pack) = &mut self.pack_info.pack {
            if pack.modified == 0 {
                pack.inserted = 0;
            } else {
                let new_len = pack.inserted;
                while pack.inserted > 0 {
                    let new_end =
                        std::cmp::min(pack.inserted + pack.modified - 1, self.dense.len());
                    self.dense.swap(new_end, pack.inserted - 1);
                    self.data.swap(new_end, pack.inserted - 1);
                    pack.inserted -= 1;
                }
                for i in pack.modified.saturating_sub(new_len)..pack.modified + new_len {
                    unsafe {
                        *self
                            .sparse
                            .get_unchecked_mut(self.dense.get_unchecked(i).index()) = i;
                    }
                }
            }
        }
    }
    pub fn clear_modified(&mut self) {
        if let Pack::Update(pack) = &mut self.pack_info.pack {
            pack.modified = 0;
        }
    }
    pub fn clear_inserted_and_modified(&mut self) {
        if let Pack::Update(pack) = &mut self.pack_info.pack {
            pack.inserted = 0;
            pack.modified = 0;
        }
    }
    pub(crate) fn is_unique(&self) -> bool {
        self.window().is_unique()
    }
    //          ▼ old end of pack
    //              ▼ new end of pack
    // [_ _ _ _ | _ | _ _ _ _ _]
    //            ▲       ▼
    //            ---------
    //              pack
    pub(crate) fn pack(&mut self, entity: EntityId) {
        self.window_mut().pack(entity)
    }
    pub(crate) fn unpack(&mut self, entity: EntityId) {
        self.window_mut().unpack(entity)
    }
    /// Place the unique component in the storage.
    /// The storage has to be completely empty.
    pub(crate) fn insert_unique(&mut self, component: T) {
        assert!(self.sparse.is_empty() && self.dense.is_empty() && self.data.is_empty());
        self.data.push(component)
    }
    pub(crate) fn clone_indices(&self) -> Vec<EntityId> {
        self.dense.clone()
    }
}

impl<T> std::ops::Index<EntityId> for SparseSet<T> {
    type Output = T;
    fn index(&self, entity: EntityId) -> &Self::Output {
        self.get(entity).unwrap()
    }
}

impl<T> std::ops::IndexMut<EntityId> for SparseSet<T> {
    fn index_mut(&mut self, entity: EntityId) -> &mut Self::Output {
        self.get_mut(entity).unwrap()
    }
}

#[test]
fn insert() {
    let mut array = SparseSet::default();
    let mut entity_id = EntityId::zero();
    entity_id.set_index(0);
    assert!(array.insert("0", entity_id).is_none());
    entity_id.set_index(1);
    assert!(array.insert("1", entity_id).is_none());
    assert_eq!(array.len(), 2);
    entity_id.set_index(0);
    assert_eq!(array.get(entity_id), Some(&"0"));
    entity_id.set_index(1);
    assert_eq!(array.get(entity_id), Some(&"1"));
    entity_id.set_index(5);
    assert!(array.insert("5", entity_id).is_none());
    assert_eq!(array.get_mut(entity_id), Some(&mut "5"));
    entity_id.set_index(4);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(6);
    assert_eq!(array.get(entity_id), None);
    assert!(array.insert("6", entity_id).is_none());
    entity_id.set_index(5);
    assert_eq!(array.get(entity_id), Some(&"5"));
    entity_id.set_index(6);
    assert_eq!(array.get_mut(entity_id), Some(&mut "6"));
    entity_id.set_index(4);
    assert_eq!(array.get(entity_id), None);
}
#[test]
fn remove() {
    let mut array = SparseSet::default();
    let mut entity_id = EntityId::zero();
    entity_id.set_index(0);
    array.insert("0", entity_id);
    entity_id.set_index(5);
    array.insert("5", entity_id);
    entity_id.set_index(10);
    array.insert("10", entity_id);
    entity_id.set_index(0);
    assert_eq!(array.remove(entity_id), Some("0"));
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(5);
    assert_eq!(array.get(entity_id), Some(&"5"));
    entity_id.set_index(10);
    assert_eq!(array.get(entity_id), Some(&"10"));
    assert_eq!(array.remove(entity_id), Some("10"));
    entity_id.set_index(0);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(5);
    assert_eq!(array.get(entity_id), Some(&"5"));
    entity_id.set_index(10);
    assert_eq!(array.get(entity_id), None);
    assert_eq!(array.len(), 1);
    entity_id.set_index(3);
    array.insert("3", entity_id);
    entity_id.set_index(10);
    array.insert("100", entity_id);
    entity_id.set_index(0);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(3);
    assert_eq!(array.get(entity_id), Some(&"3"));
    entity_id.set_index(5);
    assert_eq!(array.get(entity_id), Some(&"5"));
    entity_id.set_index(10);
    assert_eq!(array.get(entity_id), Some(&"100"));
    entity_id.set_index(3);
    assert_eq!(array.remove(entity_id), Some("3"));
    entity_id.set_index(0);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(3);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(5);
    assert_eq!(array.get(entity_id), Some(&"5"));
    entity_id.set_index(10);
    assert_eq!(array.get(entity_id), Some(&"100"));
    assert_eq!(array.remove(entity_id), Some("100"));
    entity_id.set_index(0);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(3);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(5);
    assert_eq!(array.get(entity_id), Some(&"5"));
    entity_id.set_index(10);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(5);
    assert_eq!(array.remove(entity_id), Some("5"));
    entity_id.set_index(0);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(3);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(5);
    assert_eq!(array.get(entity_id), None);
    entity_id.set_index(10);
    assert_eq!(array.get(entity_id), None);
    assert_eq!(array.len(), 0);
}