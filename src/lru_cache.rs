use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::num::NonZeroUsize;
use std::ptr::{NonNull, drop_in_place};
use std::{fmt, mem, ptr};

struct KeyRef<K> {
    k: *const K,
}

impl<K: Hash> Hash for KeyRef<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        unsafe { (*self.k).hash(state) }
    }
}

impl<K: PartialEq> PartialEq for KeyRef<K> {
    fn eq(&self, other: &Self) -> bool {
        unsafe { (*self.k).eq(&*other.k) }
    }
}

impl<K: Eq> Eq for KeyRef<K> {}

// This type exists to allow a "blanket" Borrow implementation for KeyRef
// without conflicting with the stdlib blanket implementation.
#[repr(transparent)]
struct KeyWrapper<K: ?Sized>(K);

impl<K: ?Sized> KeyWrapper<K> {
    fn from_ref(key: &K) -> &Self {
        // SAFETY: KeyWrapper is transparent, so casting the ref like this is allowable
        unsafe { &*(key as *const K as *const KeyWrapper<K>) }
    }
}

impl<K: ?Sized + Hash> Hash for KeyWrapper<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

impl<K: ?Sized + PartialEq> PartialEq for KeyWrapper<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<K: ?Sized + Eq> Eq for KeyWrapper<K> {}

impl<K, Q> Borrow<KeyWrapper<Q>> for KeyRef<K>
where
    K: Borrow<Q>,
    Q: ?Sized,
{
    fn borrow(&self) -> &KeyWrapper<Q> {
        let key = unsafe { &*self.k }.borrow();
        KeyWrapper::from_ref(key)
    }
}

struct Node<K, V> {
    key: MaybeUninit<K>,
    val: MaybeUninit<V>,
    // prev <-- node --> next
    prev: *mut Node<K, V>,
    next: *mut Node<K, V>,
}

impl<K, V> Node<K, V> {
    fn new(key: K, val: V) -> Node<K, V> {
        Node {
            key: MaybeUninit::new(key),
            val: MaybeUninit::new(val),
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
        }
    }

    fn uninit() -> Node<K, V> {
        Node {
            key: MaybeUninit::uninit(),
            val: MaybeUninit::uninit(),
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
        }
    }
}

struct LruCache<K, V> {
    map: HashMap<KeyRef<K>, NonNull<Node<K, V>>>,
    cap: NonZeroUsize,
    head: *mut Node<K, V>,
    tail: *mut Node<K, V>,
}

impl<K: Hash + Eq, V> LruCache<K, V> {
    pub fn new(cap: NonZeroUsize) -> LruCache<K, V> {
        let cache = LruCache {
            map: HashMap::with_capacity(cap.get()),
            cap,
            head: Box::into_raw(Box::new(Node::uninit())),
            tail: Box::into_raw(Box::new(Node::uninit())),
        };
        unsafe {
            (*cache.head).next = cache.tail;
            (*cache.tail).prev = cache.head;
        }
        cache
    }

    // Puts a key-value pair into cache. If the key already exists in the cache, then it updates
    // the key's value and returns the old value. Otherwise, `None` is returned.
    pub fn put(&mut self, k: K, v: V) -> Option<V> {
        self.capturing_put(k, v, false).map(|(_, v)| v)
    }

    // Pushes a key-value pair into the cache. If an entry with key `k` already exists in
    // the cache or another cache entry is removed (due to the LRU capacity),
    // then it returns the old entry's key-value pair. Otherwise, returns `None`.
    pub fn push(&mut self, k: K, v: V) -> Option<(K, V)> {
        self.capturing_put(k, v, true)
    }

    // Used internally by `put` and `push` to add a new entry to the LRU.
    // Takes ownership of and returns entries replaced due to the cache's capacity
    // when `capture` is true.
    fn capturing_put(&mut self, k: K, mut v: V, capture: bool) -> Option<(K, V)> {
        let node_ref = self.map.get_mut(&KeyRef { k: &k });
        match node_ref {
            Some(node_ref) => {
                // if the key is already in the cache just update its value and move it to the
                // front of the list
                let node_ptr: *mut Node<K, V> = node_ref.as_ptr();
                // gets a reference to the node to perform a swap and drops it right after
                let old_val_ref = unsafe { &mut *(*node_ptr).val.as_mut_ptr() };
                mem::swap(&mut v, old_val_ref);
                let _ = old_val_ref;
                self.move_to_front(node_ptr);
                Some((k, v))
            }
            None => {
                let (replaced_kv, node) = self.replace_or_create(k, v);
                let node_ptr = node.as_ptr();
                self.attach(node_ptr);
                let key_ref = KeyRef {
                    k: unsafe { &*(*node_ptr).key.as_ptr() },
                };
                self.map.insert(key_ref, node);
                replaced_kv.filter(|_| capture)
            }
        }
    }

    // Used internally to swap out a node if the cache is full or to create a new node if space
    // is available. Shared between `put`, `push`, `get_or_insert`, and `get_or_insert_mut`.
    fn replace_or_create(&mut self, k: K, v: V) -> (Option<(K, V)>, NonNull<Node<K, V>>) {
        if self.len() == self.cap.get() {
            // if the cache is full, remove the last entry so we can use it for the new key
            let old_node_key = KeyRef {
                k: unsafe { &*(*(*self.tail).prev).key.as_ptr() },
            };
            let old_node = self.map.remove(&old_node_key).unwrap();
            let old_node_ptr = old_node.as_ptr();
            // read out the node's old key and value and then replace it
            let replaced_kv = unsafe {
                (
                    mem::replace(&mut (*old_node_ptr).key, MaybeUninit::new(k)).assume_init(),
                    mem::replace(&mut (*old_node_ptr).val, MaybeUninit::new(v)).assume_init(),
                )
            };
            self.detach(old_node_ptr);
            // old node is with updated key and value
            (Some(replaced_kv), old_node)
        } else {
            // if the cache is not full allocate a new Node.
            // Safety: We allocate, turn into raw, and get NonNull all in one step.
            (None, unsafe {
                NonNull::new_unchecked(Box::into_raw(Box::new(Node::new(k, v))))
            })
        }
    }

    // Returns a reference to the value of the key in the cache or `None` if it is not
    // present in the cache. Moves the key to the head of the LRU list if it exists.
    pub fn get<Q>(&mut self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            Some(unsafe { &*(*node_ptr).val.as_ptr() })
        } else {
            None
        }
    }

    // Returns a mutable reference to the value of the key in the cache or `None` if it
    // is not present in the cache. Moves the key to the head of the LRU list if it exists.
    pub fn get_mut<Q>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            Some(unsafe { &mut *(*node_ptr).val.as_mut_ptr() })
        } else {
            None
        }
    }

    // Returns a key-value references pair of the key in the cache or `None` if it is not
    // present in the cache. Moves the key to the head of the LRU list if it exists.
    pub fn get_key_value<Q>(&mut self, k: &Q) -> Option<(&K, &V)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            Some(unsafe { (&*(*node_ptr).key.as_ptr(), &*(*node_ptr).val.as_ptr()) })
        } else {
            None
        }
    }

    // Returns a key-value references pair of the key in the cache or `None` if it is not
    // present in the cache. The reference to the value of the key is mutable. Moves the key to
    // the head of the LRU list if it exists.
    pub fn get_key_value_mut<Q>(&mut self, k: &Q) -> Option<(&K, &mut V)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            Some(unsafe {
                (
                    &*(*node_ptr).key.as_ptr(),
                    &mut *(*node_ptr).val.as_mut_ptr(),
                )
            })
        } else {
            None
        }
    }

    // Returns a reference to the value of the key in the cache if it is
    // present in the cache and moves the key to the head of the LRU list.
    // If the key does not exist the provided `FnOnce` is used to populate
    // the list and a reference is returned.
    pub fn get_or_insert<F>(&mut self, k: K, f: F) -> &V
    where
        F: FnOnce() -> V,
    {
        if let Some(node) = self.map.get_mut(&KeyRef { k: &k }) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            unsafe { &*(*node_ptr).val.as_ptr() }
        } else {
            let v = f();
            let (_, node) = self.replace_or_create(k, v);
            let node_ptr = node.as_ptr();
            self.attach(node_ptr);
            let key_ref = KeyRef {
                k: unsafe { &*(*node_ptr).key.as_ptr() },
            };
            self.map.insert(key_ref, node);
            unsafe { &*(*node_ptr).val.as_ptr() }
        }
    }

    // Returns a reference to the value of the key in the cache if it is
    // present in the cache and moves the key to the head of the LRU list.
    // If the key does not exist the provided `FnOnce` is used to populate
    // the list and a reference is returned. The value referenced by the
    // key is only cloned (using `to_owned()`) if it doesn't exist in the
    // cache.
    pub fn get_or_insert_ref<Q, F>(&mut self, k: &Q, f: F) -> &V
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized + ToOwned<Owned = K>,
        F: FnOnce() -> V,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            unsafe { &*(*node_ptr).val.as_ptr() }
        } else {
            let v = f();
            let (_, node) = self.replace_or_create(k.to_owned(), v);
            let node_ptr = node.as_ptr();
            self.attach(node_ptr);
            let key_ref = KeyRef {
                k: unsafe { &*(*node_ptr).key.as_ptr() },
            };
            self.map.insert(key_ref, node);
            unsafe { &*(*node_ptr).val.as_ptr() }
        }
    }

    // Returns a reference to the value of the key in the cache if it is
    // present in the cache and moves the key to the head of the LRU list.
    // If the key does not exist the provided `FnOnce` is used to populate
    // the list and a reference is returned. If `FnOnce` returns `Err`,
    // returns the `Err`.
    pub fn try_get_or_insert<F, E>(&mut self, k: K, f: F) -> Result<&V, E>
    where
        F: FnOnce() -> Result<V, E>,
    {
        if let Some(node) = self.map.get_mut(&KeyRef { k: &k }) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            Ok(unsafe { &*(*node_ptr).val.as_ptr() })
        } else {
            let v = f()?;
            let (_, node) = self.replace_or_create(k, v);
            let node_ptr = node.as_ptr();
            self.attach(node_ptr);
            let key_ref = KeyRef {
                k: unsafe { &*(*node_ptr).key.as_ptr() },
            };
            self.map.insert(key_ref, node);
            Ok(unsafe { &*(*node_ptr).val.as_ptr() })
        }
    }

    // Returns a reference to the value of the key in the cache if it is
    // present in the cache and moves the key to the head of the LRU list.
    // If the key does not exist the provided `FnOnce` is used to populate
    // the list and a reference is returned. If `FnOnce` returns `Err`,
    // returns the `Err`. The value referenced by the key is only cloned
    // (using `to_owned()`) if it doesn't exist in the cache and `FnOnce`
    // succeeds.
    pub fn try_get_or_insert_ref<Q, F, E>(&mut self, k: &Q, f: F) -> Result<&V, E>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized + ToOwned<Owned = K>,
        F: FnOnce() -> Result<V, E>,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            Ok(unsafe { &*(*node_ptr).val.as_ptr() })
        } else {
            let v = f()?;
            let (_, node) = self.replace_or_create(k.to_owned(), v);
            let node_ptr = node.as_ptr();
            self.attach(node_ptr);
            let key_ref = KeyRef {
                k: unsafe { &*(*node_ptr).key.as_ptr() },
            };
            self.map.insert(key_ref, node);
            Ok(unsafe { &*(*node_ptr).val.as_ptr() })
        }
    }

    // Returns a mutable reference to the value of the key in the cache if it is
    // present in the cache and moves the key to the head of the LRU list.
    // If the key does not exist the provided `FnOnce` is used to populate
    // the list and a mutable reference is returned.
    pub fn get_or_insert_mut<F>(&mut self, k: K, f: F) -> &mut V
    where
        F: FnOnce() -> V,
    {
        if let Some(node) = self.map.get_mut(&KeyRef { k: &k }) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            unsafe { &mut *(*node_ptr).val.as_mut_ptr() }
        } else {
            let v = f();
            let (_, node) = self.replace_or_create(k, v);
            let node_ptr = node.as_ptr();
            self.attach(node_ptr);
            let key_ref = KeyRef {
                k: unsafe { &*(*node_ptr).key.as_ptr() },
            };
            self.map.insert(key_ref, node);
            unsafe { &mut *(*node_ptr).val.as_mut_ptr() }
        }
    }

    // Returns a mutable reference to the value of the key in the cache if it is
    // present in the cache and moves the key to the head of the LRU list.
    // If the key does not exist the provided `FnOnce` is used to populate
    // the list and a mutable reference is returned. The value referenced by the
    // key is only cloned (using `to_owned()`) if it doesn't exist in the cache.
    pub fn get_or_insert_mut_ref<Q, F>(&mut self, k: &Q, f: F) -> &mut V
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized + ToOwned<Owned = K>,
        F: FnOnce() -> V,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            unsafe { &mut *(*node_ptr).val.as_mut_ptr() }
        } else {
            let v = f();
            let (_, node) = self.replace_or_create(k.to_owned(), v);
            let node_ptr = node.as_ptr();
            self.attach(node_ptr);
            let key_ref = KeyRef {
                k: unsafe { &*(*node_ptr).key.as_ptr() },
            };
            self.map.insert(key_ref, node);
            unsafe { &mut *(*node_ptr).val.as_mut_ptr() }
        }
    }

    // Returns a mutable reference to the value of the key in the cache if it is
    // present in the cache and moves the key to the head of the LRU list.
    // If the key does not exist the provided `FnOnce` is used to populate
    // the list and a mutable reference is returned. If `FnOnce` returns `Err`,
    // returns the `Err`.
    pub fn try_get_or_insert_mut<F, E>(&mut self, k: K, f: F) -> Result<&mut V, E>
    where
        F: FnOnce() -> Result<V, E>,
    {
        if let Some(node) = self.map.get_mut(&KeyRef { k: &k }) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            Ok(unsafe { &mut *(*node_ptr).val.as_mut_ptr() })
        } else {
            let v = f()?;
            let (_, node) = self.replace_or_create(k, v);
            let node_ptr = node.as_ptr();
            self.attach(node_ptr);
            let key_ref = KeyRef {
                k: unsafe { &*(*node_ptr).key.as_ptr() },
            };
            self.map.insert(key_ref, node);
            Ok(unsafe { &mut *(*node_ptr).val.as_mut_ptr() })
        }
    }

    // Returns a mutable reference to the value of the key in the cache if it is
    // present in the cache and moves the key to the head of the LRU list.
    // If the key does not exist the provided `FnOnce` is used to populate
    // the list and a mutable reference is returned. If `FnOnce` returns `Err`,
    // returns the `Err`. The value referenced by the key is only cloned
    // (using `to_owned()`) if it doesn't exist in the cache and `FnOnce`
    // succeeds.
    pub fn try_get_or_insert_mut_ref<Q, F, E>(&mut self, k: &Q, f: F) -> Result<&mut V, E>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized + ToOwned<Owned = K>,
        F: FnOnce() -> Result<V, E>,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.move_to_front(node_ptr);
            Ok(unsafe { &mut *(*node_ptr).val.as_mut_ptr() })
        } else {
            let v = f()?;
            let (_, node) = self.replace_or_create(k.to_owned(), v);
            let node_ptr = node.as_ptr();
            self.attach(node_ptr);
            let key_ref = KeyRef {
                k: unsafe { &*(*node_ptr).key.as_ptr() },
            };
            self.map.insert(key_ref, node);
            Ok(unsafe { &mut *(*node_ptr).val.as_mut_ptr() })
        }
    }

    // Returns a reference to the value corresponding to the key in the cache or `None` if it is
    // not present in the cache. Unlike `get`, `peek` does not update the LRU list so the key's
    // position will be unchanged.
    // Returns a reference to the value of the key in the cache or `None` if it is not
    // present in the cache. Moves the key to the head of the LRU list if it exists.
    pub fn peek<Q>(&mut self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map
            .get(KeyWrapper::from_ref(k))
            .map(|node| unsafe { &*node.as_ref().val.as_ptr() })
    }

    // Returns a mutable reference to the value corresponding to the key in the cache or `None`
    // if it is not present in the cache. Unlike `get_mut`, `peek_mut` does not update the LRU
    // list so the key's position will be unchanged.
    pub fn peek_mut<Q>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map
            .get_mut(KeyWrapper::from_ref(k))
            .map(|node| unsafe { &mut *(*node.as_ptr()).val.as_mut_ptr() })
    }

    // Returns the value corresponding to the least recently used item or `None` if the
    // cache is empty. Like `peek`, `peek_lru` does not update the LRU list so the item's
    // position will be unchanged.
    pub fn peek_lru(&self) -> Option<(&K, &V)> {
        if self.is_empty() {
            return None;
        }
        let (key, val);
        unsafe {
            let node = (*self.tail).prev;
            key = &*(*node).key.as_ptr();
            val = &*(*node).val.as_ptr();
        }
        Some((key, val))
    }

    // Returns the value corresponding to the most recently used item or `None` if the
    // cache is empty. Like `peek`, `peek_mru` does not update the LRU list so the item's
    // position will be unchanged.
    pub fn peek_mru(&self) -> Option<(&K, &V)> {
        if self.is_empty() {
            return None;
        }
        let (key, val);
        unsafe {
            let node = (*self.head).next;
            key = &*(*node).key.as_ptr();
            val = &*(*node).val.as_ptr();
        }
        Some((key, val))
    }

    // Removes and returns the value corresponding to the key from the cache or
    // `None` if it does not exist.
    pub fn pop<Q>(&mut self, k: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.remove(KeyWrapper::from_ref(k)).map(|old_node| {
            let mut old_node = unsafe {
                let mut old_node = *Box::from_raw(old_node.as_ptr());
                drop_in_place(old_node.key.as_mut_ptr());
                old_node
            };
            self.detach(&mut old_node);
            unsafe { old_node.val.assume_init() }
        })
    }

    // Removes and returns the key and the value corresponding to the key from the cache or
    // `None` if it does not exist.
    pub fn pop_entry<Q>(&mut self, k: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.remove(KeyWrapper::from_ref(k)).map(|old_node| {
            let mut old_node = unsafe { *Box::from_raw(old_node.as_ptr()) };
            self.detach(&mut old_node);
            let Node { key, val, .. } = old_node;
            unsafe { (key.assume_init(), val.assume_init()) }
        })
    }

    // Removes and returns the key and value corresponding to the least recently
    // used item or `None` if the cache is empty.
    pub fn pop_lru(&mut self) -> Option<(K, V)> {
        let node = self.remove_last()?;
        let node = *node;
        let Node { key, val, .. } = node;
        Some(unsafe { (key.assume_init(), val.assume_init()) })
    }

    // Removes and returns the key and value corresponding to the most recently
    // used item or `None` if the cache is empty.
    pub fn pop_mru(&mut self) -> Option<(K, V)> {
        let node = self.remove_first()?;
        let node = *node;
        let Node { key, val, .. } = node;
        Some(unsafe { (key.assume_init(), val.assume_init()) })
    }

    // Marks the key as the most recently used one. Returns true if the key
    // was promoted because it exists in the cache, false otherwise.
    pub fn promote<Q>(&mut self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.detach(node_ptr);
            self.attach(node_ptr);
            return true;
        }
        false
    }

    pub fn demote<Q>(&mut self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(node) = self.map.get_mut(KeyWrapper::from_ref(k)) {
            let node_ptr = node.as_ptr();
            self.detach(node_ptr);
            self.attach_last(node_ptr);
            return true;
        }
        false
    }

    fn remove_first(&mut self) -> Option<Box<Node<K, V>>> {
        let next = unsafe { (*self.head).next };
        if !ptr::eq(next, self.tail) {
            let old_key = KeyRef {
                k: unsafe { &*(*next).key.as_ptr() },
            };
            let old_node = self.map.remove(&old_key).unwrap();
            let old_node_ptr = old_node.as_ptr();
            self.detach(old_node_ptr);
            Some(unsafe { Box::from_raw(old_node_ptr) })
        } else {
            None
        }
    }

    fn remove_last(&mut self) -> Option<Box<Node<K, V>>> {
        let prev = unsafe { (*self.tail).prev };
        if !ptr::eq(prev, self.head) {
            let old_key = KeyRef {
                k: unsafe { &*(*prev).key.as_ptr() },
            };
            let old_node = self.map.remove(&old_key).unwrap();
            let old_node_ptr = old_node.as_ptr();
            self.detach(old_node_ptr);
            Some(unsafe { Box::from_raw(old_node_ptr) })
        } else {
            None
        }
    }

    fn move_to_front(&mut self, node: *mut Node<K, V>) {
        self.detach(node);
        self.attach(node);
    }

    // Removes node.
    fn detach(&mut self, node: *mut Node<K, V>) {
        unsafe {
            (*(*node).prev).next = (*node).next;
            (*(*node).next).prev = (*node).prev;
        }
    }

    // Attaches node after head node.
    fn attach(&mut self, node: *mut Node<K, V>) {
        unsafe {
            (*node).prev = self.head;
            (*node).next = (*self.head).next;
            (*(*node).next).prev = node;
            (*self.head).next = node;
        }
    }

    // Attaches node before tail node.
    fn attach_last(&mut self, node: *mut Node<K, V>) {
        unsafe {
            (*node).prev = (*self.tail).prev;
            (*node).next = self.tail;
            (*(*node).prev).next = node;
            (*self.tail).prev = node;
        }
    }

    // Resizes the cache. If the new capacity is smaller than the size of the current
    // cache any entries past the new capacity are discarded.
    pub fn resize(&mut self, cap: NonZeroUsize) {
        if cap == self.cap {
            return;
        }
        while self.map.len() > cap.get() {
            self.pop_lru();
        }
        self.map.shrink_to_fit();
        self.cap = cap;
    }

    // Clears the contents of the cache.
    pub fn clear(&mut self) {
        while self.pop_lru().is_some() {}
    }

    // Returns a bool indicating whether the given key is in the cache. Does not update the
    // LRU list.
    pub fn contains<Q>(&mut self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.contains_key(KeyWrapper::from_ref(k))
    }

    pub fn is_empty(&self) -> bool {
        self.map.len() == 0
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn cap(&self) -> NonZeroUsize {
        self.cap
    }

    // An iterator visiting all entries in most-recently used order.
    // The iterator element type is `(&K, &V)`.
    pub fn iter(&self) -> Iter<K, V> {
        Iter {
            ptr: unsafe { (*self.head).next },
            end: unsafe { (*self.tail).prev },
            len: self.len(),
            _phantom: PhantomData,
        }
    }

    // An iterator visiting all entries in most-recently-used order,
    // giving a mutable reference to V.
    // The iterator element type is `(&K, &mut V)`.
    pub fn iter_mut(&self) -> IterMut<K, V> {
        IterMut {
            ptr: unsafe { (*self.head).next },
            end: unsafe { (*self.tail).prev },
            len: self.len(),
            _phantom: PhantomData,
        }
    }
}

impl<K, V> Drop for LruCache<K, V> {
    fn drop(&mut self) {
        self.map.drain().for_each(|(_, node)| unsafe {
            let mut node = *Box::from_raw(node.as_ptr());
            drop_in_place(node.key.as_mut_ptr());
            drop_in_place(node.val.as_mut_ptr());
        });
        // re-box the head/tail, and because these are maybe-uninit
        // they do not have the absent k/v dropped
        let _head = unsafe { *Box::from_raw(self.head) };
        let _tail = unsafe { *Box::from_raw(self.tail) };
    }
}

impl<K, V> Clone for LruCache<K, V>
where
    K: Hash + PartialEq + Eq + Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        let mut new_lru = LruCache::new(self.cap);
        for (key, val) in self.iter().rev() {
            new_lru.push(key.clone(), val.clone());
        }
        new_lru
    }
}

impl<'a, K: Hash + Eq, V> IntoIterator for &'a LruCache<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// An iterator over the entries of a `LruCache`.
pub struct Iter<'a, K: 'a, V: 'a> {
    ptr: *const Node<K, V>,
    end: *const Node<K, V>,
    len: usize,
    _phantom: PhantomData<&'a K>,
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.len == 0 {
            return None;
        }
        let key = unsafe { &*(*self.ptr).key.as_ptr() };
        let val = unsafe { &*(*self.ptr).val.as_ptr() };
        self.ptr = unsafe { (*self.ptr).next };
        self.len -= 1;
        Some((key, val))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }

    fn count(self) -> usize {
        self.len
    }
}

impl<'a, K, V> DoubleEndedIterator for Iter<'a, K, V> {
    fn next_back(&mut self) -> Option<(&'a K, &'a V)> {
        if self.len == 0 {
            return None;
        }
        let key = unsafe { &*(*self.end).key.as_ptr() };
        let val = unsafe { &*(*self.end).val.as_ptr() };
        self.ptr = unsafe { (*self.ptr).prev };
        self.len -= 1;
        Some((key, val))
    }
}

impl<'a, K, V> Clone for Iter<'a, K, V> {
    fn clone(&self) -> Iter<'a, K, V> {
        Iter {
            len: self.len,
            ptr: self.ptr,
            end: self.end,
            _phantom: PhantomData,
        }
    }
}

impl<'a, K, V> ExactSizeIterator for Iter<'a, K, V> {}

impl<'a, K, V> FusedIterator for Iter<'a, K, V> {}

// The compiler does not automatically derive Send and Sync for Iter because it contains
// raw pointers.
unsafe impl<'a, K: Send, V: Send> Send for Iter<'a, K, V> {}

unsafe impl<'a, K: Sync, V: Sync> Sync for Iter<'a, K, V> {}

impl<'a, K: Hash + Eq, V> IntoIterator for &'a mut LruCache<K, V> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = IterMut<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

// An iterator over mutable entries of a `LruCache`.
pub struct IterMut<'a, K: 'a, V: 'a> {
    ptr: *mut Node<K, V>,
    end: *mut Node<K, V>,
    len: usize,
    _phantom: PhantomData<&'a K>,
}

impl<'a, K, V> Iterator for IterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.len == 0 {
            return None;
        }
        let key = unsafe { &*(*self.ptr).key.as_ptr() };
        let val = unsafe { &mut *(*self.ptr).val.as_mut_ptr() };
        self.ptr = unsafe { (*self.ptr).next };
        self.len -= 1;
        Some((key, val))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }

    fn count(self) -> usize {
        self.len
    }
}

impl<'a, K, V> DoubleEndedIterator for IterMut<'a, K, V> {
    fn next_back(&mut self) -> Option<(&'a K, &'a mut V)> {
        if self.len == 0 {
            return None;
        }
        let key = unsafe { &*(*self.end).key.as_ptr() };
        let val = unsafe { &mut *(*self.end).val.as_mut_ptr() };
        self.ptr = unsafe { (*self.ptr).prev };
        self.len -= 1;
        Some((key, val))
    }
}

impl<'a, K, V> ExactSizeIterator for IterMut<'a, K, V> {}

impl<'a, K, V> FusedIterator for IterMut<'a, K, V> {}

// The compiler does not automatically derive Send and Sync for Iter because it contains
// raw pointers.
unsafe impl<'a, K: Send, V: Send> Send for IterMut<'a, K, V> {}

unsafe impl<'a, K: Sync, V: Sync> Sync for IterMut<'a, K, V> {}

impl<K: Hash + Eq, V> IntoIterator for LruCache<K, V> {
    type Item = (K, V);
    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter { cache: self }
    }
}

// An iterator that moves out of a `LruCache`.
pub struct IntoIter<K: Hash + Eq, V> {
    cache: LruCache<K, V>,
}

impl<K: Hash + Eq, V> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.cache.pop_lru()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.cache.len();
        (len, Some(len))
    }

    fn count(self) -> usize {
        self.cache.len()
    }
}

impl<K, V> ExactSizeIterator for IntoIter<K, V> where K: Hash + Eq {}

impl<K, V> FusedIterator for IntoIter<K, V> where K: Hash + Eq {}

// The compiler does not automatically derive Send and Sync for LruCache because it contains
// raw pointers. The raw pointers are safely encapsulated by LruCache though so we can
// implement Send and Sync for it below.
unsafe impl<K: Send, V: Send> Send for LruCache<K, V> {}

unsafe impl<K: Sync, V: Sync> Sync for LruCache<K, V> {}

impl<K: Hash + Eq, V> fmt::Debug for LruCache<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LruCache")
            .field("len", &self.len())
            .field("cap", &self.cap())
            .finish()
    }
}
