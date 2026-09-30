#[cfg(not(target_vendor = "apple"))]
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub(super) struct Sha256Prefix {
    #[cfg(target_vendor = "apple")]
    state: AppleSha256Context,
    #[cfg(not(target_vendor = "apple"))]
    state: Sha256,
}

impl Sha256Prefix {
    pub(super) fn new(bytes: &[u8]) -> Option<Self> {
        #[cfg(target_vendor = "apple")]
        let mut prefix = {
            let mut state = AppleSha256Context::default();
            // SAFETY: the initialized context has the public CommonDigest.h layout.
            if unsafe { CC_SHA256_Init(&mut state) } != HASH_OPERATION_SUCCEEDED {
                return None;
            }
            Self { state }
        };
        #[cfg(not(target_vendor = "apple"))]
        let mut prefix = Self {
            state: Sha256::new(),
        };
        prefix.update(bytes)?;
        Some(prefix)
    }

    pub(super) fn finish(&self, suffix: &[u8]) -> Option<[u8; SHA256_DIGEST_BYTES]> {
        let mut combined = self.clone();
        combined.update(suffix)?;
        #[cfg(target_vendor = "apple")]
        {
            let mut digest = [0; SHA256_DIGEST_BYTES];
            // SAFETY: both buffers are live and the output holds a full SHA-256 digest.
            if unsafe { CC_SHA256_Final(digest.as_mut_ptr(), &mut combined.state) }
                != HASH_OPERATION_SUCCEEDED
            {
                return None;
            }
            Some(digest)
        }
        #[cfg(not(target_vendor = "apple"))]
        Some(combined.state.finalize().into())
    }

    fn update(&mut self, bytes: &[u8]) -> Option<()> {
        #[cfg(target_vendor = "apple")]
        for chunk in bytes.chunks(u32::MAX as usize) {
            // SAFETY: chunk covers the supplied CC_LONG length; the context is initialized.
            if unsafe {
                CC_SHA256_Update(&mut self.state, chunk.as_ptr().cast(), chunk.len() as u32)
            } != HASH_OPERATION_SUCCEEDED
            {
                return None;
            }
        }
        #[cfg(not(target_vendor = "apple"))]
        self.state.update(bytes);
        Some(())
    }
}

const SHA256_DIGEST_BYTES: usize = 32;

#[cfg(target_vendor = "apple")]
const HASH_OPERATION_SUCCEEDED: std::ffi::c_int = 1;

// Public CC_SHA256_CTX from CommonCrypto/CommonDigest.h. Zeroing also initializes
// unused buffer words before a prefix context is cloned.
#[cfg(target_vendor = "apple")]
#[repr(C)]
#[derive(Clone, Debug, Default)]
struct AppleSha256Context {
    count: [u32; 2],
    hash: [u32; 8],
    wbuf: [u32; 16],
}

#[cfg(target_vendor = "apple")]
#[link(name = "System")]
unsafe extern "C" {
    fn CC_SHA256_Init(context: *mut AppleSha256Context) -> std::ffi::c_int;
    fn CC_SHA256_Update(
        context: *mut AppleSha256Context,
        data: *const std::ffi::c_void,
        len: u32,
    ) -> std::ffi::c_int;
    fn CC_SHA256_Final(digest: *mut u8, context: *mut AppleSha256Context) -> std::ffi::c_int;
}

#[cfg(test)]
mod tests {
    use super::Sha256Prefix;
    use sha2::{Digest, Sha256};

    #[test]
    fn cloned_prefixes_match_full_hash_at_padding_and_block_boundaries() {
        const CORPUS_BYTES: usize = 1025;
        let bytes: Vec<u8> = (0..CORPUS_BYTES).map(|index| index as u8).collect();
        for split in [0, 1, 55, 56, 63, 64, 65, 127, 128, 129, CORPUS_BYTES] {
            let prefix = Sha256Prefix::new(&bytes[..split]).unwrap();
            for end in [split, (split + bytes.len()) / 2, bytes.len()] {
                let expected: [u8; 32] = Sha256::digest(&bytes[..end]).into();
                assert_eq!(
                    prefix.finish(&bytes[split..end]),
                    Some(expected),
                    "split={split}, end={end}"
                );
            }
            assert_eq!(
                prefix.finish(&bytes[split..]),
                Some(super::super::canonical_sha256(&bytes)),
                "finalizing a clone must not mutate the retained prefix at {split}"
            );
        }
    }
}
