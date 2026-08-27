//! Wallet-side helpers for proving spends of Tachyon notes.

extern crate alloc;

use alloc::vec::Vec;

use ragu::{Pcd, Proof};
use rand_core::CryptoRng;

use crate::{
    EpochIndex,
    keys::{GGM_CHUNK_SIZE, GGM_TREE_DEPTH, ProofAuthorizingKey},
    note::Note,
    stamp::{
        ProofStamp,
        proof::{PROOF_SYSTEM, delegation, spendable},
    },
    witness,
};

/// The private proof inputs corresponding to one planned spend action.
///
/// Pass these inputs to [`crate::StampPlan::prove`] in the same order as the
/// spend actions in the bundle plan.
pub type SpendProofInputs = (
    Pcd<delegation::NullifierHeader>,
    Pcd<spendable::SpendableHeader>,
);

/// Prepare proof inputs for spending a note in the epoch where it was created.
///
/// `creation_stamp` must be the proof stamp that revealed the note commitment.
/// The returned spendable is rooted immediately after that stamp, and the
/// nullifier proof covers `epoch` and its successor as required by a spend
/// stamp.
///
/// Notes spent in a later epoch must first have their spendable proof lifted
/// across the intervening anchor and nullifier-exclusion history.
pub fn spend_inputs_for_created_note<RNG: CryptoRng>(
    rng: &mut RNG,
    note: &Note,
    pak: &ProofAuthorizingKey,
    creation_stamp: &ProofStamp,
    epoch: EpochIndex,
) -> Result<SpendProofInputs, ragu::Error> {
    if !creation_stamp
        .tachygrams
        .contains(&note.commitment().into())
    {
        return Err(ragu::Error::InvalidWitness(
            "note commitment is absent from its creation stamp".into(),
        ));
    }

    let master = note_master(rng, note, pak)?;
    let creation_nullifier = nullifier_range(rng, &master, epoch, 1)?;
    let present_nullifier = creation_nullifier.data().1.1;
    let (spendable, ()) = PROOF_SYSTEM.fuse(
        rng,
        spendable::SpendableInit,
        witness::spendable_init(
            (*creation_nullifier.data(), ()),
            creation_stamp.anchor,
            &creation_stamp
                .tachygrams
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            present_nullifier,
        ),
        creation_nullifier,
        Proof::trivial().carry::<()>(()),
    )?;
    let nullifier_range = nullifier_range(rng, &master, epoch, 2)?;

    Ok((nullifier_range, spendable))
}

fn note_master<RNG: CryptoRng>(
    rng: &mut RNG,
    note: &Note,
    pak: &ProofAuthorizingKey,
) -> Result<Pcd<delegation::NfPrefixHeader>, ragu::Error> {
    PROOF_SYSTEM
        .seed(rng, delegation::NfMasterSeed, (*note, *pak))
        .map(|(pcd, ())| pcd)
}

fn nullifier_range<RNG: CryptoRng>(
    rng: &mut RNG,
    master: &Pcd<delegation::NfPrefixHeader>,
    epoch_start: EpochIndex,
    len: u32,
) -> Result<Pcd<delegation::NullifierHeader>, ragu::Error> {
    let mut nullifiers = Vec::new();
    let mut accumulated: Option<Pcd<delegation::NullifierHeader>> = None;

    for offset in 0..len {
        let epoch = EpochIndex(epoch_start.0.checked_add(offset).ok_or_else(|| {
            ragu::Error::InvalidWitness("nullifier range exceeds the maximum epoch".into())
        })?);
        let prefix = walk_to_epoch(rng, master.clone(), epoch)?;
        let (leaf, ()) = PROOF_SYSTEM.fuse(
            rng,
            delegation::NullifierStep,
            (),
            prefix,
            Proof::trivial().carry::<()>(()),
        )?;
        let nullifier = leaf.data().1.1;

        accumulated = Some(match accumulated {
            None => leaf,
            Some(left) => {
                let fuse_witness =
                    witness::nullifier_fuse((*left.data(), *leaf.data()), &nullifiers, nullifier);
                PROOF_SYSTEM
                    .fuse(rng, delegation::NullifierFuse, fuse_witness, left, leaf)?
                    .0
            },
        });
        nullifiers.push(nullifier);
    }

    accumulated
        .ok_or_else(|| ragu::Error::InvalidWitness("nullifier range must not be empty".into()))
}

fn walk_to_epoch<RNG: CryptoRng>(
    rng: &mut RNG,
    mut prefix: Pcd<delegation::NfPrefixHeader>,
    epoch: EpochIndex,
) -> Result<Pcd<delegation::NfPrefixHeader>, ragu::Error> {
    while prefix.data().2 < GGM_TREE_DEPTH {
        let next_depth = prefix.data().2 + 1;
        let chunk = epoch_chunk(epoch, next_depth)?;
        prefix = PROOF_SYSTEM
            .fuse(
                rng,
                delegation::NfPrefixStep,
                (chunk,),
                prefix,
                Proof::trivial().carry::<()>(()),
            )?
            .0;
    }

    Ok(prefix)
}

fn epoch_chunk(epoch: EpochIndex, depth: u8) -> Result<u8, ragu::Error> {
    let shift = (GGM_TREE_DEPTH * GGM_CHUNK_SIZE) - depth * GGM_CHUNK_SIZE;
    let chunk_mask = (1u32 << GGM_CHUNK_SIZE) - 1;
    let chunk = (epoch.0 >> shift) & chunk_mask;
    u8::try_from(chunk)
        .map_err(|_error| ragu::Error::InvalidWitness("GGM epoch chunk does not fit in u8".into()))
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use rand::{SeedableRng as _, rngs::StdRng};

    use crate::{
        Anchor, action, bundle,
        entropy::ActionEntropy,
        fixtures::{WalletSim, build_output_stamp},
        primitives::effect,
        value,
    };

    use super::*;

    #[test]
    fn created_note_inputs_prove_a_spend() {
        let rng = &mut StdRng::seed_from_u64(0);
        let wallet = WalletSim::random(rng);
        let note = wallet.random_note(500);
        let epoch = EpochIndex(3);
        let (creation_stamp, _) = build_output_stamp(rng, Anchor::default(), note);
        let creation_anchor = creation_stamp
            .anchor
            .next_stamp(epoch, &creation_stamp.tachygram_set)
            .expect("test stamp advances the anchor");
        let inputs = spend_inputs_for_created_note(rng, &note, &wallet.pak, &creation_stamp, epoch)
            .expect("created note has spend proof inputs");
        let theta = ActionEntropy::random(rng);
        let spend = action::Plan::<effect::Spend>::spend(
            note,
            theta,
            value::Trapdoor::random(rng),
            |alpha| wallet.pak.ak.derive_action_public(&alpha),
        );
        let digest = spend.descriptor().digest().expect("valid spend digest");
        let stamp = bundle::Plan::new(vec![spend], vec![])
            .stamp_plan(creation_anchor)
            .prove(rng, &wallet.pak, vec![inputs])
            .expect("spend proof succeeds");

        assert_eq!(stamp.anchor, creation_anchor);
        assert!(
            stamp
                .verify_proof(rng, [digest])
                .expect("spend proof verifies")
        );
    }
}
