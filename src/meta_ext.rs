use frame_metadata::{
    v12::{DecodeDifferent, ModuleMetadata, StorageEntryMetadata, StorageMetadata},
    RuntimeMetadata, RuntimeMetadataPrefixed,
};

pub trait MetaExt<'a> {
    type EntriesIter: Iterator<Item = &'a StorageMetadata>;

    fn storage_entries(&'a self) -> Self::EntriesIter;

    fn entry(&'a self, module: &str, name: &str) -> Option<&StorageEntryMetadata> {
        self.storage_entries().find_map(|s| {
            if s.prefix.decoded().eq(module) {
                Some(
                    s.entries
                        .decoded()
                        .iter()
                        .find(|e| e.name.decoded() == name),
                )
            } else {
                None
            }
            .flatten()
        })
    }

    fn from_bytes(bytes: Vec<u8>) -> Result<RuntimeMetadata, scale::Error> {
        scale::Decode::decode(&mut bytes.as_slice()).map(|m: RuntimeMetadataPrefixed| m.1)
    }
}

// can't do `impl Iterator` on traits yet
type FilteredEntries<'a> = core::iter::FilterMap<
    core::slice::Iter<'a, ModuleMetadata>,
    fn(&ModuleMetadata) -> Option<&StorageMetadata>,
>;

impl<'a> MetaExt<'a> for RuntimeMetadata {
    type EntriesIter = FilteredEntries<'a>;

    fn storage_entries(&'a self) -> Self::EntriesIter {
        match self {
            RuntimeMetadata::V12(meta) => meta
                .modules
                .decoded()
                .iter()
                .filter_map(|m| m.storage.as_ref().map(|s| s.decoded())),
            _ => unreachable!(),
        }
    }
}

trait Decoded {
    type Output;
    fn decoded(&self) -> &Self::Output;
}

impl<B, O> Decoded for DecodeDifferent<B, O> {
    type Output = O;
    fn decoded(&self) -> &Self::Output {
        match self {
            DecodeDifferent::Decoded(o) => o,
            _ => unreachable!(),
        }
    }
}
