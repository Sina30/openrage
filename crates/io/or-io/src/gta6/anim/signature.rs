//! An animation's signature at 0x2C: a Fletcher checksum over its DOFs.

use super::{Animation, Dof};

/// DOFs the sums take before both fold to 16 bits.
const RUN: usize = 180;

/// The signature an animation of `dofs`, in their stored order, states at 0x2C.
// Per DOF the words `track << 8 | type | 0x80` and `id` go into two wrapping u32 sums from 0xFFFF.
pub fn dof_signature(dofs: &[Dof]) -> u32 {
    let fold = |sum: u32| (sum & 0xFFFF) + (sum >> 16);
    let (mut s1, mut s2) = (0xFFFFu32, 0xFFFFu32);
    for run in dofs.chunks(RUN) {
        for dof in run {
            for word in [dof.track << 8 | dof.kind | 0x80, dof.id] {
                s1 = s1.wrapping_add(word);
                s2 = s2.wrapping_add(s1);
            }
        }
        s1 = fold(s1);
        s2 = fold(s2);
    }
    fold(s2) << 16 | fold(s1)
}

impl Animation {
    /// Whether the signature at 0x2C is the one its DOFs give.
    pub fn signature_holds(&self) -> bool {
        dof_signature(&self.dofs) == self.signature
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dof(id: u32, track: u32, kind: u32) -> Dof {
        Dof { id, track, kind }
    }

    /// The sums in u64, reduced to 32 bits after every addition.
    fn reference(dofs: &[Dof]) -> u32 {
        let wrap = |v: u64| v & 0xFFFF_FFFF;
        let fold = |v: u64| (v & 0xFFFF) + (v >> 16);
        let (mut s1, mut s2) = (0xFFFFu64, 0xFFFFu64);
        for (i, d) in dofs.iter().enumerate() {
            let first = wrap((u64::from(d.track) << 8) | u64::from(d.kind) | 0x80);
            s1 = wrap(s1 + first);
            s2 = wrap(s2 + s1);
            s1 = wrap(s1 + u64::from(d.id));
            s2 = wrap(s2 + s1);
            if (i + 1) % RUN == 0 || i + 1 == dofs.len() {
                (s1, s2) = (fold(s1), fold(s2));
            }
        }
        wrap(fold(s2) << 16 | fold(s1)) as u32
    }

    #[test]
    fn the_signature_sums_each_dofs_words_and_folds_every_180() {
        assert_eq!(dof_signature(&[]), 0xFFFF_FFFF);
        let dofs: Vec<Dof> = (0..400u32)
            .map(|i| dof(0x9E37_79B9u32.wrapping_mul(i + 1), 0xC2B2_AE35 ^ i, i % 5))
            .collect();
        for n in [1, 2, 179, 180, 181, 360, 400] {
            assert_eq!(dof_signature(&dofs[..n]), reference(&dofs[..n]), "{n} DOFs");
        }
    }

    #[test]
    fn the_signature_tells_a_dof_list_from_its_reordering_and_from_another_type() {
        let list = [dof(0x11, 0x2222_0001, 0), dof(0x33, 0x4444_0002, 2)];
        let swapped = [list[1], list[0]];
        let retyped = [list[0], dof(0x33, 0x4444_0002, 3)];
        let signature = dof_signature(&list);
        assert_ne!(signature, dof_signature(&swapped));
        assert_ne!(signature, dof_signature(&retyped));
        let animation = Animation {
            dofs: list.to_vec(),
            signature,
            ..Animation::default()
        };
        assert!(animation.signature_holds());
        let other = Animation {
            signature: signature ^ 1,
            ..animation
        };
        assert!(!other.signature_holds());
    }
}
