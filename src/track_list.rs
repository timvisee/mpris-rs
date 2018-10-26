extern crate dbus;
use super::{DBusError, Metadata, Player};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::iter::{FromIterator, IntoIterator};

/// Represents [the MPRIS `Track_Id` type][track_id].
///
/// ```rust
/// use mpris::TrackID;
/// let no_track = TrackID::new("/org/mpris/MediaPlayer2/TrackList/NoTrack").unwrap();
/// ```
///
/// TrackIDs must be valid D-Bus object paths according to the spec.
///
/// # Errors
///
/// Trying to construct a `TrackID` from a string that is not a valid D-Bus Path will fail.
///
/// ```rust
/// # use mpris::TrackID;
/// let result = TrackID::new("invalid track ID");
/// assert!(result.is_err());
/// ```
///
/// [track_id]: https://specifications.freedesktop.org/mpris-spec/latest/Player_Interface.html#Simple-Type:Track_Id
#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct TrackID(pub(crate) String);

/// Represents a MediaPlayer2.TrackList.
///
/// This type offers an iterator of the track's metadata, when provided a `Player` instance that
/// matches the list.
///
/// TrackLists cache metadata about tracks so multiple iterations should be fast. It also enables
/// signals received from the Player to pre-populate metadata and to keep everything up to date.
///
/// See [MediaPlayer2.TrackList
/// interface](https://specifications.freedesktop.org/mpris-spec/latest/Track_List_Interface.html)
#[derive(Debug, Default)]
pub struct TrackList {
    ids: Vec<TrackID>,
    metadata_cache: RefCell<HashMap<TrackID, Metadata>>,
}

/// TrackList-related errors.
///
/// This is mostly `DBusError` with the extra possibility of borrow errors of the internal metadata
/// cache.
#[derive(Debug, Fail)]
pub enum TrackListError {
    /// Something went wrong with the D-Bus communication. See the `DBusError` type.
    #[fail(display = "D-Bus communication failed")]
    DBusError(#[cause] DBusError),

    /// Something went wrong with the borrowing logic for the internal cache. Perhaps you have
    /// multiple borrowed references to the cache live at the same time, for example because of
    /// multiple iterations?
    #[fail(display = "Could not borrow cache: {}", _0)]
    BorrowError(String),
}

#[derive(Debug)]
pub struct MetadataIter {
    order: Vec<TrackID>,
    metadata: HashMap<TrackID, Metadata>,
    current: usize,
}

impl<'a> From<dbus::Path<'a>> for TrackID {
    fn from(path: dbus::Path<'a>) -> TrackID {
        TrackID(path.to_string())
    }
}

impl<'a> From<&'a TrackID> for TrackID {
    fn from(id: &'a TrackID) -> Self {
        TrackID(id.0.clone())
    }
}

impl From<TrackID> for String {
    fn from(id: TrackID) -> String {
        id.0
    }
}

impl fmt::Display for TrackID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl TrackID {
    /// Create a new `TrackID` from a string-like entity.
    ///
    /// This is not something you should normally do as the IDs are temporary and will only work if
    /// the Player knows about it.
    ///
    /// However, creating `TrackID`s manually can help with test setup, comparisons, etc.
    ///
    /// # Example
    /// ```rust
    /// use mpris::TrackID;
    /// let id = TrackID::new("/dbus/path/id").expect("Parse error");
    /// ```
    pub fn new<S: Into<String>>(id: S) -> Result<Self, String> {
        let id = id.into();
        // Validate the ID by constructing a dbus::Path.
        if let Err(error) = dbus::Path::new(id.as_str()) {
            Err(error)
        } else {
            Ok(TrackID(id))
        }
    }

    /// Returns a `&str` variant of the ID.
    pub fn as_str(&self) -> &str {
        &*self.0
    }

    pub(crate) fn as_path(&self) -> dbus::Path {
        // All inputs to this class should be validated to work with dbus::Path, so unwrapping
        // should be safe here.
        dbus::Path::new(self.as_str()).unwrap()
    }
}

impl From<Vec<TrackID>> for TrackList {
    fn from(ids: Vec<TrackID>) -> Self {
        TrackList::new(ids)
    }
}

impl<'a> From<Vec<dbus::Path<'a>>> for TrackList {
    fn from(ids: Vec<dbus::Path<'a>>) -> Self {
        ids.into_iter().map(TrackID::from).collect()
    }
}

impl FromIterator<TrackID> for TrackList {
    fn from_iter<I: IntoIterator<Item = TrackID>>(iter: I) -> Self {
        TrackList::new(iter.into_iter().collect())
    }
}

impl TrackList {
    /// Construct a new TrackList without any existing cache.
    pub fn new(ids: Vec<TrackID>) -> TrackList {
        TrackList {
            metadata_cache: RefCell::new(HashMap::with_capacity(ids.len())),
            ids,
        }
    }

    /// Get a list of TrackIDs that are part of this TrackList. The order matters.
    pub fn ids(&self) -> Vec<&TrackID> {
        self.ids.iter().collect()
    }

    /// Returns the number of tracks on the list.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Insert a new track after another one. If the provided ID cannot be found on the list, it
    /// will be inserted at the end.
    pub fn insert(&mut self, after: &TrackID, metadata: Metadata) {
        let new_id = match metadata.track_id() {
            Some(val) => val,
            // Cannot insert ID if there is no ID in the metadata.
            None => return,
        };

        let index = self
            .ids
            .iter()
            .enumerate()
            .find(|(_, id)| *id == after)
            .map(|(index, _)| index)
            .unwrap_or_else(|| self.ids.len());

        // Vec::insert inserts BEFORE the given index, but we need to insert *after* the index.
        if index >= self.ids.len() {
            self.ids.push(new_id.clone());
        } else {
            self.ids.insert(index + 1, new_id.clone());
        }

        // borrow_mut should be safe as we have a &mut self, so no one else may have borrowed this
        // cache.
        self.metadata_cache.borrow_mut().insert(new_id, metadata);
    }

    /// Removes a track from the list and metadata cache.
    pub fn remove(&mut self, id: &TrackID) {
        self.ids.retain(|existing_id| existing_id != id);

        // borrow_mut should be safe as we have a &mut self, so no one else may have borrowed this
        // cache.
        self.metadata_cache.borrow_mut().remove(id);
    }

    /// Replace the contents with the contents of the provided list. Cache will be reused when
    /// possible.
    pub fn replace(&mut self, other: TrackList) {
        self.ids = other.ids;

        // borrow_mut should be safe as we have a &mut self and a owned value, so no one else may have borrowed this
        // cache.
        let mut self_cache = self.metadata_cache.borrow_mut();
        let other_cache = other.metadata_cache.into_inner();
        // Will overwrite existing keys = cache in "other_cache" will win on conflict.
        self_cache.extend(other_cache.into_iter());
    }

    /// Updates the metadata cache for the given `TrackID`.
    ///
    /// The metadata will be added to the cache even if the `TrackID` isn't part of the list, but
    /// will be cleaned out again after the next cache cleanup unless the track in question have
    /// been added to the list before then.
    pub fn update_metadata(&mut self, id: &TrackID, metadata: Metadata) {
        // borrow_mut should be safe as we have a &mut self, so no one else may have borrowed this
        // cache.
        self.metadata_cache
            .borrow_mut()
            .insert(id.to_owned(), metadata);
    }

    /// Iterates the tracks in the tracklist, returning a tuple of TrackID and Metadata for that
    /// track.
    ///
    /// If metadata loading fails, then a DBusError will be returned instead.
    pub fn metadata_iter(&self, player: &Player) -> Result<MetadataIter, TrackListError> {
        self.complete_cache(player)?;
        let metadata: HashMap<_, _> = self.metadata_cache.clone().into_inner();
        let ids = self.ids.clone();

        Ok(MetadataIter {
            current: 0,
            order: ids,
            metadata,
        })
    }

    /// Reloads the tracklist from the given player. This can be compared with loading a new track
    /// list, but in this case the metadata cache can be maintained for tracks that remain on the
    /// list.
    ///
    /// Cache for tracks that are no longer part of the player's tracklist will be removed.
    pub fn reload(&mut self, player: &Player) -> Result<(), TrackListError> {
        self.ids = player.get_track_list()?.ids;
        self.clear_extra_cache();
        Ok(())
    }

    /// Clears all cache and reloads metadata for all tracks.
    ///
    /// Cache will be replaced *after* the new metadata has been loaded, so on load errors the
    /// cache will still be maintained.
    pub fn reload_cache(&self, player: &Player) -> Result<(), TrackListError> {
        let id_metadata = self
            .ids
            .iter()
            .cloned()
            .zip(player.get_tracks_metadata(&self.ids)?);
        let mut cache = self.metadata_cache.borrow_mut();
        *cache = id_metadata.collect();
        Ok(())
    }

    /// Fill in any holes in the cache so that each track on the list has a cached Metadata entry.
    ///
    /// If all tracks already have a cache entry, then this will do nothing.
    pub fn complete_cache(&self, player: &Player) -> Result<(), TrackListError> {
        let ids: Vec<_> = self
            .ids_without_cache()
            .into_iter()
            .map(Clone::clone)
            .collect();
        if !ids.is_empty() {
            let metadata = player.get_tracks_metadata(&ids)?;

            let mut cache = self.metadata_cache.try_borrow_mut()?;
            for info in metadata.into_iter() {
                match info.track_id() {
                    Some(id) => {
                        cache.insert(id, info);
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }

    fn ids_without_cache(&self) -> Vec<&TrackID> {
        let cache = &*self.metadata_cache.borrow();
        self.ids
            .iter()
            .filter(|id| !cache.contains_key(id))
            .collect()
    }

    fn clear_extra_cache(&mut self) {
        // &mut self means that no other reference to self exists, so it should always be safe to
        // mutably borrow the cache.
        let mut cache = self.metadata_cache.borrow_mut();

        // For each id in the list, move the cache out into a new HashMap, then replace the old
        // one with the new. Only ids on the list will therefore be present on the new list.
        let new_cache: HashMap<TrackID, Metadata> = self
            .ids
            .iter()
            .flat_map(|id| match cache.remove(&id) {
                Some(value) => Some((id.clone(), value)),
                None => None,
            }).collect();

        *cache = new_cache;
    }
}

impl Iterator for MetadataIter {
    type Item = Metadata;

    fn next(&mut self) -> Option<Self::Item> {
        match self.order.get(self.current) {
            Some(next_id) => {
                self.current += 1;
                // In case of race conditions with cache population, emit a simple Metadata without
                // any interesting data in it.
                Some(
                    self.metadata
                        .remove(next_id)
                        .unwrap_or_else(|| Metadata::new(next_id.clone())),
                )
            }
            None => None,
        }
    }
}

impl From<DBusError> for TrackListError {
    fn from(error: DBusError) -> TrackListError {
        TrackListError::DBusError(error)
    }
}

impl From<::std::cell::BorrowMutError> for TrackListError {
    fn from(error: ::std::cell::BorrowMutError) -> TrackListError {
        TrackListError::BorrowError(format!("Could not borrow mutably: {}", error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track_id(s: &str) -> TrackID {
        TrackID::new(s).expect("Failed to parse a TrackID fixture")
    }

    mod track_list {
        use super::*;

        #[test]
        fn it_inserts_after_given_id() {
            let first = track_id("/path/1");
            let third = track_id("/path/3");

            let mut list = TrackList {
                ids: vec![first, third],
                metadata_cache: RefCell::new(HashMap::new()),
            };

            let metadata = Metadata::new("/path/new");
            list.insert(&track_id("/path/1"), metadata);

            assert_eq!(list.len(), 3);
            assert_eq!(
                &list.ids,
                &[
                    track_id("/path/1"),
                    track_id("/path/new"),
                    track_id("/path/3")
                ]
            );
            assert_eq!(
                list.ids_without_cache(),
                vec![&track_id("/path/1"), &track_id("/path/3")],
            );
        }

        #[test]
        fn it_inserts_at_end_on_missing_id() {
            let first = track_id("/path/1");
            let third = track_id("/path/3");

            let mut list = TrackList {
                ids: vec![first, third],
                metadata_cache: RefCell::new(HashMap::new()),
            };

            let metadata = Metadata::new("/path/new");
            list.insert(&track_id("/path/missing"), metadata);

            assert_eq!(list.len(), 3);
            assert_eq!(
                &list.ids,
                &[
                    track_id("/path/1"),
                    track_id("/path/3"),
                    track_id("/path/new"),
                ]
            );
            assert_eq!(
                list.ids_without_cache(),
                vec![&track_id("/path/1"), &track_id("/path/3")],
            );
        }
    }
}
