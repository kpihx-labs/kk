use super::*;
use crate::{
    Identity,
    rooms::{Grant, Rights, enrollment::Authority, mls::PublicIdentity},
};

impl Group {
    pub(crate) fn enrollment_authority(
        &self,
        identity: &Identity,
        creator: PublicIdentity,
        subject: &Peer,
        rights: Rights,
        seeds: Vec<crate::rooms::enrollment::Seed>,
    ) -> Result<Authority> {
        self.ready()?;
        if identity.peer().fingerprint() != self.actor.owner {
            return Err(Error::Authentication);
        }
        if !self
            .member(self.state.own_leaf_index())?
            .rights()
            .can_invite()
        {
            return Err(Error::Denied);
        }
        let public = self.actor.public_identity()?;
        let mut grants = public.authority_chain(&self.actor.anchor)?;
        grants.push(Grant::issue(
            &self.actor.anchor,
            &grants,
            identity,
            subject,
            rights,
        )?);
        Authority::issued(identity, &self.actor.anchor, creator, public, grants, seeds)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::rooms::{Anchor, RootBundle};
    fn seed(identity: &Identity) -> Vec<crate::rooms::enrollment::Seed> {
        vec![crate::rooms::enrollment::Seed {
            certificate: identity.peer().certificate.as_ref().to_vec(),
            endpoint: "127.0.0.1:45461".parse().unwrap(),
        }]
    }

    fn admitted(
        rights: Rights,
    ) -> (
        tempfile::TempDir,
        Identity,
        Identity,
        Anchor,
        PublicIdentity,
        Group,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let a = Identity::create(&directory.path().join("a")).unwrap();
        let b = Identity::create(&directory.path().join("b")).unwrap();
        let anchor = RootBundle::create(&a, "Enrollment authority ceiling")
            .unwrap()
            .verify(&a.peer())
            .unwrap();
        let creator = CryptoMember::new(&a, &anchor, vec![]).unwrap();
        let origin = creator.public_identity().unwrap();
        let mut owner = Group::create(creator).unwrap();
        let grant = Grant::issue(&anchor, &[], &a, &b.peer(), rights).unwrap();
        let mut candidate = CryptoMember::new(&b, &anchor, vec![grant]).unwrap();
        let package = candidate.key_package().unwrap();
        let validated = owner.validate_package(&package).unwrap();
        let commit = owner.prepare_add(validated).unwrap();
        let welcome = owner.confirm_ordered(1, &commit).unwrap().unwrap();
        let joined = Group::join(candidate, &welcome, &a.peer(), 1).unwrap();
        (directory, a, b, anchor, origin, joined)
    }
    #[test]
    fn delegated_authority_verifies_after_creator_departure_without_its_private_key() {
        let (directory, a, b, anchor, origin, mut joined) = admitted(Rights::MODERATOR);
        let c = Identity::create(&directory.path().join("c")).unwrap();
        let remove = joined.prepare_remove(&a.peer().fingerprint()).unwrap();
        joined.confirm_ordered(2, &remove).unwrap();
        drop(a);
        let authority = joined
            .enrollment_authority(&b, origin, &c.peer(), Rights::PARTICIPANT, seed(&b))
            .unwrap();
        let received = Authority::decode(&authority.encode().unwrap()).unwrap();
        let verified = received
            .verify(anchor.id(), &b.peer(), &c.peer(), Rights::PARTICIPANT)
            .unwrap();
        let mut candidate = verified
            .create_candidate(&c, &directory.path().join("private-c"))
            .unwrap();
        assert!(candidate.key_package().is_ok());
        assert!(
            received
                .verify(anchor.id(), &b.peer(), &c.peer(), Rights::MODERATOR)
                .is_err()
        );
    }
    #[test]
    fn a_reader_cannot_issue_bootstrap_authority_even_with_its_own_device_key() {
        let (directory, _a, b, _anchor, origin, joined) = admitted(Rights::READER);
        let c = Identity::create(&directory.path().join("c")).unwrap();
        assert!(matches!(
            joined.enrollment_authority(&b, origin, &c.peer(), Rights::READER, seed(&b)),
            Err(Error::Denied)
        ));
    }
}
