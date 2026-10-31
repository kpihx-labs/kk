//! Persist the EXACT signed nonce/proposal with the pending crypto transaction.
//! A crash or uncertain submission must not invent a new operation identity.
use super::*;
use crate::{
    Identity,
    rooms::control::{Proposal, View},
};

const PREPARED: &str = "prepared_control";

impl Group {
    /// `view` must be obtained from authoritative fresh control, not a remote DTO.
    pub fn prepare_admission(
        &mut self,
        identity: &Identity,
        view: &View,
        package: &[u8],
    ) -> Result<Proposal> {
        self.proposal_context(identity, view)?;
        let candidate = self.validate_package(package)?;
        self.transactional(|group| {
            let commit = group.prepare_add_mut(candidate)?;
            let welcome = group
                .pending
                .as_ref()
                .and_then(|pending| pending.welcome.as_deref())
                .ok_or(Error::Protocol)?;
            let proposal = Proposal::add_with_welcome(
                identity,
                group.room_id(),
                group.epoch(),
                view.head(),
                &commit,
                package,
                welcome,
            )?;
            group.save_prepared_control(&proposal)?;
            Ok(proposal)
        })
    }
    pub fn prepare_departure(
        &mut self,
        identity: &Identity,
        view: &View,
        peer: &Peer,
    ) -> Result<Proposal> {
        self.proposal_context(identity, view)?;
        self.transactional(|group| {
            let commit = group.prepare_remove_mut(&peer.fingerprint())?;
            let proposal = Proposal::remove(
                identity,
                group.room_id(),
                group.epoch(),
                view.head(),
                &commit,
                peer,
            )?;
            group.save_prepared_control(&proposal)?;
            Ok(proposal)
        })
    }
    pub fn prepared_control(&self) -> Result<Option<Proposal>> {
        self.ready()?;
        Self::load_prepared_control(&self.actor, self.pending.as_ref(), self.epoch())
    }
    /// Backend-only verified recovery metadata. Admission identity comes from the
    /// existing MLS pending Add, bound to the package in the signed operation;
    /// no new parsing of untrusted payloads or revalidation of package lifetime.
    pub(crate) fn prepared_control_with_member(
        &self,
    ) -> Result<Option<(Proposal, super::super::control::raft::PeerId)>> {
        let Some(proposal) = self.prepared_control()? else {
            return Ok(None);
        };
        let member = match proposal.action()? {
            super::super::control::Action::Remove => {
                super::super::control::raft::PeerId(proposal.removed().ok_or(Error::Protocol)?)
            }
            super::super::control::Action::Add => {
                let staged = self.state.pending_commit().ok_or(Error::Authentication)?;
                let mut adds =
                    staged
                        .queued_proposals()
                        .filter_map(|queued| match queued.proposal() {
                            openmls::prelude::Proposal::Add(add) => Some(add),
                            _ => None,
                        });
                let add = adds.next().ok_or(Error::Authentication)?;
                if adds.next().is_some() || staged.remove_proposals().next().is_some() {
                    return Err(Error::Authentication);
                }
                let package = add.key_package();
                if package
                    .tls_serialize_detached()
                    .map_err(|_| Error::Protocol)?
                    .as_slice()
                    != proposal.package_bytes().ok_or(Error::Protocol)?
                {
                    return Err(Error::Authentication);
                }
                let leaf = package.leaf_node();
                let verified = verify_credential(
                    &self.actor.anchor,
                    leaf.credential(),
                    leaf.signature_key().as_slice(),
                )?;
                super::super::control::raft::PeerId::from_peer(&verified.peer())
            }
        };
        Ok(Some((proposal, member)))
    }
    fn proposal_context(&self, identity: &Identity, view: &View) -> Result<()> {
        self.ready()?;
        if identity.peer().fingerprint() != self.actor.owner || view.epoch() != self.epoch() {
            return Err(Error::Authentication);
        }
        self.check_transition_view(view.membership_digest()?)
    }
    fn save_prepared_control(&mut self, proposal: &Proposal) -> Result<()> {
        let pending = self.pending.as_mut().ok_or(Error::Protocol)?;
        if pending.digest != <[u8; 32]>::from(Sha256::digest(proposal.commit_bytes())) {
            return Err(Error::Authentication);
        }
        self.actor
            .provider
            .write_blob(PREPARED, &proposal.encode()?)?;
        pending.proposal = true;
        Ok(())
    }
    pub(super) fn load_prepared_control(
        actor: &CryptoMember,
        pending: Option<&Pending>,
        epoch: u64,
    ) -> Result<Option<Proposal>> {
        let stored = actor.provider.read_blob(PREPARED)?;
        let expected = pending.is_some_and(|pending| pending.proposal);
        if stored.is_some() != expected {
            return Err(Error::Storage(
                "pending proposal and private checkpoint disagree",
            ));
        }
        let Some(bytes) = stored else {
            return Ok(None);
        };
        let proposal = Proposal::decode(&bytes)?;
        if pending.is_none_or(|pending| {
            pending.digest != <[u8; 32]>::from(Sha256::digest(proposal.commit_bytes()))
        }) {
            return Err(Error::Authentication);
        }
        let own = verify_credential(
            &actor.anchor,
            &actor.credential.credential,
            actor.credential.signature_key.as_slice(),
        )?;
        proposal.verify(actor.anchor.id(), epoch, proposal.previous(), &own.peer())?;
        // Both copies must describe the same authenticated operation. Otherwise
        // a corrupted checkpoint could replace/drop the deliverable while the
        // signed proposal and pending commit still pass their independent checks.
        if pending.and_then(|pending| pending.welcome.as_deref()) != proposal.encrypted_welcome() {
            return Err(Error::Storage(
                "pending Welcome and signed proposal disagree",
            ));
        }
        Ok(Some(proposal))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::rooms::{Grant, Rights, RootBundle};

    #[test]
    fn pending_proposal_survives_restart_and_exact_rejection_without_reusing_package() {
        let directory = crate::test_support::tempdir().unwrap();
        let alice = Identity::create(&directory.path().join("alice")).unwrap();
        let bob = Identity::create(&directory.path().join("bob")).unwrap();
        let anchor = RootBundle::create(&alice, "Pending restart fixture")
            .unwrap()
            .verify(&alice.peer())
            .unwrap();
        let path = directory.path().join("private-a");
        let actor = CryptoMember::create_private(&alice, &anchor, vec![], &path).unwrap();
        let view = View::genesis(&anchor, actor.public_identity().unwrap()).unwrap();
        let mut group = Group::create(actor).unwrap();
        let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::PARTICIPANT).unwrap();
        let mut candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
        let package = candidate.key_package().unwrap();
        let proposal = group.prepare_admission(&alice, &view, &package).unwrap();
        let exact = proposal.encode().unwrap();
        drop(group);

        let actor = CryptoMember::open_private(&alice, &anchor, &path).unwrap();
        let mut restored = Group::restore(actor).unwrap();
        assert_eq!(
            restored.prepared_control_with_member().unwrap().unwrap().1,
            super::super::super::control::raft::PeerId::from_peer(&bob.peer())
        );
        assert_eq!(
            restored
                .prepared_control()
                .unwrap()
                .unwrap()
                .encode()
                .unwrap(),
            exact
        );
        // An unrelated/stale rejection must neither clear nor replace the nonce.
        assert!(restored.reject_ordered(b"not the prepared commit").is_err());
        assert_eq!(
            restored
                .prepared_control()
                .unwrap()
                .unwrap()
                .encode()
                .unwrap(),
            exact
        );
        assert!(
            restored
                .encrypt_text("pending blocks publication", false)
                .is_err()
        );
        restored.reject_ordered(proposal.commit_bytes()).unwrap();
        assert!(restored.prepared_control().unwrap().is_none());
        assert_eq!(restored.epoch(), 0);
        restored
            .encrypt_text("rejection releases publication", false)
            .unwrap();
        // Rejection is not permission to recycle an already used MLS package.
        assert!(restored.prepare_admission(&alice, &view, &package).is_err());
        let replacement = candidate.key_package().unwrap();
        let next = restored
            .prepare_admission(&alice, &view, &replacement)
            .unwrap();
        assert_ne!(next.id().unwrap(), proposal.id().unwrap());
        drop(restored);
        let actor = CryptoMember::open_private(&alice, &anchor, &path).unwrap();
        let restored = Group::restore(actor).unwrap();
        assert_eq!(
            restored
                .prepared_control()
                .unwrap()
                .unwrap()
                .encode()
                .unwrap(),
            next.encode().unwrap()
        );
    }

    #[test]
    fn restore_rejects_missing_or_replaced_pending_welcome_despite_valid_signed_proposal() {
        for replace in [false, true] {
            let directory = crate::test_support::tempdir().unwrap();
            let alice = Identity::create(&directory.path().join("alice")).unwrap();
            let bob = Identity::create(&directory.path().join("bob")).unwrap();
            let anchor = RootBundle::create(&alice, "Pending cache binding fixture")
                .unwrap()
                .verify(&alice.peer())
                .unwrap();
            let path = directory.path().join("private-a");
            let actor = CryptoMember::create_private(&alice, &anchor, vec![], &path).unwrap();
            let view = View::genesis(&anchor, actor.public_identity().unwrap()).unwrap();
            let mut group = Group::create(actor).unwrap();
            let grant =
                Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::PARTICIPANT).unwrap();
            let mut candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
            group
                .prepare_admission(&alice, &view, &candidate.key_package().unwrap())
                .unwrap();
            // Simulate storage corruption, not a supported mutation. The signed
            // proposal, MLS pending commit and checkpoint epoch remain intact.
            let transaction = group.actor.provider.transaction().unwrap();
            let pending = group.pending.as_mut().unwrap();
            if replace {
                pending.welcome.as_mut().unwrap()[0] ^= 1;
            } else {
                pending.welcome = None;
            }
            group.save_checkpoint().unwrap();
            transaction.commit().unwrap();
            drop(group);
            let actor = CryptoMember::open_private(&alice, &anchor, &path).unwrap();
            assert!(matches!(
                Group::restore(actor),
                Err(Error::Storage(
                    "pending Welcome and signed proposal disagree"
                ))
            ));
        }
    }

    #[test]
    fn failed_preparation_rolls_back_nonce_and_crypto_and_missing_nonce_never_regenerates() {
        let directory = crate::test_support::tempdir().unwrap();
        let alice = Identity::create(&directory.path().join("alice")).unwrap();
        let bob = Identity::create(&directory.path().join("bob")).unwrap();
        let anchor = RootBundle::create(&alice, "Pending nonce fault fixture")
            .unwrap()
            .verify(&alice.peer())
            .unwrap();
        let path = directory.path().join("private-a");
        let actor = CryptoMember::create_private(&alice, &anchor, vec![], &path).unwrap();
        let view = View::genesis(&anchor, actor.public_identity().unwrap()).unwrap();
        let mut group = Group::create(actor).unwrap();
        let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::PARTICIPANT).unwrap();
        let mut candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
        let package = candidate.key_package().unwrap();
        group.actor.provider.inject_checkpoint_commit_failure();
        assert!(matches!(
            group.prepare_admission(&alice, &view, &package),
            Err(Error::Storage(_))
        ));
        assert!(!group.is_active());
        assert!(group.prepared_control().is_err());
        drop(group);
        let actor = CryptoMember::open_private(&alice, &anchor, &path).unwrap();
        let mut restored = Group::restore(actor).unwrap();
        assert!(restored.prepared_control().unwrap().is_none());
        let proposal = restored.prepare_admission(&alice, &view, &package).unwrap();
        assert_eq!(
            restored
                .prepared_control()
                .unwrap()
                .unwrap()
                .encode()
                .unwrap(),
            proposal.encode().unwrap()
        );
        let transaction = restored.actor.provider.transaction().unwrap();
        restored.actor.provider.delete_blob(PREPARED).unwrap();
        transaction.commit().unwrap();
        drop(restored);
        let actor = CryptoMember::open_private(&alice, &anchor, &path).unwrap();
        assert!(Group::restore(actor).is_err());
    }
}
