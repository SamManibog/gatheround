// TODO
// change recursive methods into queue/stack-based methods

use crate::utils::geometry::{IRect, IVec2};

enum QuadTreeRegion {
    NE,
    SE,
    SW,
    NW,
}

/// a loose quadtree for overlap detection
/// subregions overlap slightly
#[derive(Debug)]
pub struct QuadTree<E: Clone + Eq, R: IRect> {
    /// the aabb that this quadtree occupies
    aabb: R,

    /// the total amount of entries in this tree and all subregions
    entry_count: usize,

    /// the items contained that may or may not be placed in other subregions
    contained_entries: Vec<E>,

    /// the items contained that (for sure) cannot be placed in other regions
    overflow_entries: Vec<E>,

    /// the subregions of this quadtree
    subregions: Option<Box<[QuadTree<E, R>]>>,
}

impl<E: Clone + Eq, R: IRect> QuadTree<E, R> {
    /// the number of items that this quadtree likes to have
    const TARGET_ENTRY_COUNT: usize = 4;

    /// the multiplier used to determine subregion dimensions
    /// 1/2 creates non-overlapping subregions, don't go smaller than that
    const SUBREGION_FACTOR: f32 = 5.0 / 8.0;

    pub fn new(aabb: R) -> Self {
        Self {
            aabb,
            entry_count: 0,
            contained_entries: Vec::new(),
            overflow_entries: Vec::new(),
            subregions: None,
        }
    }

    /// clears the quadtree of all contents and subregions
    pub fn clear(&mut self) {
        self.entry_count = 0;
        self.contained_entries.clear();
        self.overflow_entries.clear();
        self.subregions = None;
    }

    /// returns the aabb of the quadtree
    pub fn aabb(&self) -> R {
        self.aabb
    }

    /// gets the number of contained items in this node
    fn node_entry_count(&self) -> usize {
        self.contained_entries.len() + self.overflow_entries.len()
    }

    /// checks if an aabb is fully contained in this quadtree
    pub fn contains_aabb<S: IRect>(&self, aabb: S) -> bool {
        self.aabb.contains_rect(aabb)
    }

    /// checks if a point is fully contained in this quadtree
    pub fn contains_point<V: IVec2>(&self, point: V) -> bool {
        self.aabb.contains_point(point)
    }

    /// checks if an aabb intersects with a quadtree
    pub fn intersects_aabb<S: IRect>(&self, aabb: S) -> bool {
        self.aabb.intersects_rect(aabb)
    }

    /// gets a list of all entries intersecting with the given aabb
    /// you must provide a fetching function that maps each entry to an aabb
    pub fn query_range<S: IRect, T: IRect>(&self, aabb: S, get_aabb: &impl Fn(&E) -> T) -> Vec<E> {
        let mut intersections = Vec::new();
        self.query_range_helper(aabb, get_aabb, &mut intersections);
        intersections
    }

    /// recursive helper for query_range, adding intersections to a list
    fn query_range_helper<S: IRect, T: IRect>(
        &self,
        aabb: S,
        get_aabb: &impl Fn(&E) -> T,
        intersecting_list: &mut Vec<E>,
    ) {
        // if aabb is out of range, return early
        if !self.intersects_aabb(aabb) {
            return;
        }

        // check intersections at this level
        for entry in &self.contained_entries {
            let entry_aabb = get_aabb(entry);
            if entry_aabb.intersects_rect(aabb) {
                intersecting_list.push(entry.clone());
            }
        }
        for entry in &self.overflow_entries {
            let entry_aabb = get_aabb(entry);
            if entry_aabb.intersects_rect(aabb) {
                intersecting_list.push(entry.clone());
            }
        }

        // check intersections at subregions
        for subregion in self.subregion_iter() {
            subregion.query_range_helper(aabb, get_aabb, intersecting_list);
        }
    }

    /// gets a list of all entries containing the given point
    /// you must provide a fetching function that maps each entry to an aabb
    pub fn query_point<V: IVec2, T: IRect>(&self, point: V, get_aabb: &impl Fn(&E) -> T) -> Vec<E> {
        let mut containers = Vec::new();
        self.query_point_helper(point, get_aabb, &mut containers);
        containers
    }

    /// recursive helper for query_point, adding containers to a list
    fn query_point_helper<V: IVec2, T: IRect>(
        &self,
        point: V,
        get_aabb: &impl Fn(&E) -> T,
        containing_list: &mut Vec<E>,
    ) {
        // if aabb is out of range, return early
        if !self.contains_point(point) {
            return;
        }

        // check intersections at this level
        for entry in &self.contained_entries {
            let entry_aabb = get_aabb(entry);
            if entry_aabb.contains_point(point) {
                containing_list.push(entry.clone());
            }
        }
        for entry in &self.overflow_entries {
            let entry_aabb = get_aabb(entry);
            if entry_aabb.contains_point(point) {
                containing_list.push(entry.clone());
            }
        }

        // check intersections at subregions
        for subregion in self.subregion_iter() {
            subregion.query_point_helper(point, get_aabb, containing_list);
        }
    }

    /// removes entries in the regions that contain the given point
    /// returns a list of all entries removed
    /// used, for example, to remove a specific entry from the tree
    pub fn extract_from_regions_on_point<V: IVec2>(
        &mut self,
        point: V,
        predicate: &impl Fn(&E) -> bool,
    ) -> Vec<E> {
        let mut removals = Vec::new();
        self.extract_from_regions_on_point_helper(point, predicate, &mut removals);
        removals
    }

    /// recursive helper for query_point, adding containers to a list
    fn extract_from_regions_on_point_helper<V: IVec2>(
        &mut self,
        point: V,
        predicate: &impl Fn(&E) -> bool,
        removal_list: &mut Vec<E>,
    ) {
        // if aabb is out of range, return early
        if !self.contains_point(point) {
            return;
        }

        let initial_list_len = removal_list.len();

        // check overlaps in this node
        removal_list.extend(
            self.contained_entries
                .extract_if(.., |entry| predicate(entry)),
        );
        removal_list.extend(
            self.overflow_entries
                .extract_if(.., |entry| predicate(entry)),
        );

        // check overlaps at subregions
        for subregion in self.subregion_iter_mut() {
            subregion.extract_from_regions_on_point_helper(point, predicate, removal_list);
        }

        // update entry count
        self.entry_count -= removal_list.len() - initial_list_len;

        // attempt to prune this node
        {
            // check if pruning can/should be performed
            if self.subregions.is_none() || self.entry_count > Self::TARGET_ENTRY_COUNT {
                return;
            }

            // consume subregions
            let regions = self.subregions.take().unwrap();

            // collect all entries in subregions into this one
            for mut region in regions {
                self.contained_entries.append(&mut region.contained_entries);
                self.contained_entries.append(&mut region.overflow_entries);
            }
        }
    }

    /// inserts an entry with the given aabb, returns true on successful insert
    /// attempts to place node in this thing first, if not full, then prioritizes placement in subregions
    /// time complexity: O(logn)
    pub fn insert<T: IRect>(&mut self, entry: E, get_aabb: &impl Fn(&E) -> T) -> bool {
        let aabb = get_aabb(&entry);

        if !self.aabb.contains_rect(aabb) {
            return false;
        }

        // premptively increment entry count
        // this is fine, entry is guaranteed to be inserted at this point
        // this is also necessary to handle borrow checker requirements
        self.entry_count += 1;

        // try to put entry in this node
        if self.node_entry_count() < Self::TARGET_ENTRY_COUNT {
            if self.subregions.is_some() && self.find_containing_regions(aabb).is_empty() {
                self.overflow_entries.push(entry);
            } else {
                self.contained_entries.push(entry);
            }
            return true;
        }

        // does not fit into this node, subdivide
        self.split_quadtree();

        // attempt to redistribute contained_entries
        {
            let mut to_distribute = Vec::new();
            std::mem::swap(&mut to_distribute, &mut self.contained_entries);
            self.entry_count -= to_distribute.len();

            while !to_distribute.is_empty() {
                let entry = to_distribute.pop().unwrap();
                self.insert(entry, get_aabb);
            }
        }

        // attempt to insert in other subregions
        // sorting should reduce the chances of overflow
        for subregion in self.subregions_sorted() {
            if subregion.insert(entry.clone(), get_aabb) {
                return true;
            }
        }

        // enter this item as overflow
        self.overflow_entries.push(entry);

        // remove new split if it's empty
        if self.node_entry_count() == self.entry_count {
            self.subregions = None;
        }

        true
    }

    /// attempts to optimize this tree and its subtrees
    /// does three operations
    /// time complexity: O(nlogn) performs two O(nlogn) operations
    ///  - pruning - attempt to raise the entries in overly-sparse trees, this reduces tree depth
    ///  - redistribution - attempt to move non-overflow entries into lower levels, this improves specificity
    pub fn optimize<T: IRect>(&mut self, get_aabb: &impl Fn(&E) -> T) {
        // prune remove unnecessary depth
        self.prune();

        // redistribution NEVER creates new nodes
        self.redistribute(get_aabb);
    }

    /// attempt to raise entries to higher levels in overly-sparse trees
    /// performed in depth-first order
    /// time complexity: O(logn) constant time operations are run once per node, size of nodes is logn
    fn prune(&mut self) {
        // perform this operation postfix order
        for subregion in self.subregion_iter_mut() {
            subregion.prune();
        }

        // main logic
        // note: entry count does not need to change as no entries are added or removed from this subtree

        // we are done if there are no regions to collapse or there are more entries than can fit in this node
        if self.subregions.is_none() || self.entry_count > Self::TARGET_ENTRY_COUNT {
            return;
        }

        // consume subregions
        let regions = self.subregions.take().unwrap();

        // collect all entries in subregions into this one
        for mut region in regions {
            self.contained_entries.append(&mut region.contained_entries);
            self.contained_entries.append(&mut region.overflow_entries);
        }
    }

    /// attempt to move non-overflow entries into lower levels
    /// time complexity: O(nlogn) insertion is logn, all nodes will be reinserted
    fn redistribute<T: IRect>(&mut self, get_aabb: &impl Fn(&E) -> T) {
        // if we are not at capacity then redistribution is not helpful and we are done with
        // this node
        if self.entry_count <= Self::TARGET_ENTRY_COUNT {
            return;
        }

        // insertion on later steps will be useless if we do not split now
        self.split_quadtree();

        // first, remove and store all contained entries
        // redistribution only needs to be performed on our contained_entries because
        // by our invariants overflow_entries cannot fit in subdivisions
        let mut to_distribute = Vec::new();
        std::mem::swap(&mut to_distribute, &mut self.contained_entries);
        self.entry_count -= to_distribute.len();

        // second, redistribute entries by re-inserting them
        while !to_distribute.is_empty() {
            let entry = to_distribute.pop().unwrap();
            self.insert(entry, get_aabb);
        }

        // recurse on subregions
        for subregion in self.subregion_iter_mut() {
            subregion.redistribute(get_aabb);
        }
    }

    /// maps an index to its corresponding QuadTreeRegion, panics if the index is greater than 3
    fn indexed_region(index: usize) -> QuadTreeRegion {
        match index {
            0 => QuadTreeRegion::NE,
            1 => QuadTreeRegion::SE,
            2 => QuadTreeRegion::SW,
            3 => QuadTreeRegion::NW,
            _ => unreachable!("If invariants are met, should be unreachable."),
        }
    }

    /// returns an iterator over all subregions
    fn subregion_iter(&self) -> impl Iterator<Item = &Self> {
        if let Some(regions_list) = &self.subregions {
            regions_list.iter()
        } else {
            [].iter()
        }
    }

    /// returns a mutable iterator over all subregions
    fn subregion_iter_mut(&mut self) -> impl Iterator<Item = &mut Self> {
        if let Some(regions_list) = self.subregions.as_deref_mut() {
            regions_list.iter_mut()
        } else {
            [].iter_mut()
        }
    }

    /// returns an array of mutable subregions, sorted by contained entry count ascending
    fn subregions_sorted(&mut self) -> Vec<&mut Self> {
        if let Some(regions_list) = self.subregions.as_deref_mut() {
            let mut clone_list: Vec<&mut Self> = regions_list.iter_mut().collect();
            clone_list.sort_unstable_by_key(|f| f.entry_count);
            clone_list
        } else {
            Vec::new()
        }
    }

    /// creates the new subregions, doesn't add or move around items
    /// returns true if the split was successful (not already split)
    fn split_quadtree(&mut self) -> bool {
        if self.subregions.is_some() {
            return false;
        }

        let sub_width = self.aabb.width() * Self::SUBREGION_FACTOR;
        let sub_height = self.aabb.height() * Self::SUBREGION_FACTOR;

        // "near" x and y positions relative to the topleft corner (the topleft corner lol)
        let near_x = self.aabb.x();
        let near_y = self.aabb.y();

        // "far" x and y positions relative to the topleft corner
        let far_x = self.aabb.x() + self.aabb.width() - sub_width;
        let far_y = self.aabb.y() + self.aabb.height() - sub_height;

        self.subregions = Some(Box::new([
            Self::new(R::from_position_dimension(
                (far_x, near_y),
                (sub_width, sub_height),
            )),
            Self::new(R::from_position_dimension(
                (far_x, far_y),
                (sub_width, sub_height),
            )),
            Self::new(R::from_position_dimension(
                (near_x, far_y),
                (sub_width, sub_height),
            )),
            Self::new(R::from_position_dimension(
                (near_x, near_y),
                (sub_width, sub_height),
            )),
        ]));

        true
    }

    /// returns a list of all subregions that the given aabb would fit in
    fn find_containing_regions<S: IRect>(&self, aabb: S) -> Vec<QuadTreeRegion> {
        let mut regions = Vec::new();
        for (index, subregion) in self.subregion_iter().enumerate() {
            if subregion.contains_aabb(aabb) {
                regions.push(Self::indexed_region(index));
            }
        }
        regions
    }

    /// an iterator over all entries in this node
    pub fn entry_iter(&self) -> impl Iterator<Item = &E> {
        self.contained_entries
            .iter()
            .chain(self.overflow_entries.iter())
    }

    /// a debug function for drawing this quadtree
    /// nodes are visited with postorder traversal
    /// a quadtree aabb, otherwise false for an entry aabb
    /// draw_aabb is a function that should draw a rectangle based on the following arguments:
    ///  1) the rectangle itself
    ///  2) a reference to the entry (if the rect is for an entry, otherwise none to signify that
    ///     it's for a quadtree subregion)
    pub fn debug_draw<T: IRect + Into<R>>(
        &self,
        get_aabb: &impl Fn(&E) -> T,
        draw_aabb: &mut impl FnMut(&R, Option<&E>),
    ) {
        // draw entry aabbs
        for entry in self.entry_iter() {
            let aabb = T::into(get_aabb(entry));
            draw_aabb(&aabb, Some(entry));
        }

        // draw this aabb
        draw_aabb(&self.aabb, None);

        // recurse on subregions
        for region in self.subregion_iter() {
            region.debug_draw(get_aabb, draw_aabb);
        }
    }
}
