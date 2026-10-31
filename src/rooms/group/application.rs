//! Structured application operations share the existing MLS identity and durable
//! transaction owner. This SDK does not establish remote capability compatibility.
use super::*;
use crate::rooms::{conversation, store::history as store};

pub struct Conversation {
    pub sender: String,
    pub operation: conversation::Operation,
}

impl Group {
    /// Caller must establish that the captured audience supports this context.
    /// No wire/reference escapes before ratchet, capture and checkpoint commit.
    pub fn encrypt_conversation_referenced(
        &mut self,
        operation: &conversation::Operation,
        now: u64,
    ) -> Result<(Vec<u8>, StableReference)> {
        operation.validate_publication()?;
        self.transactional_with_tx(|group, tx| {
            group.application_ready()?;
            let plaintext = operation.encode(group.room_id(), group.state.epoch().as_u64())?;
            let enabled = group.history_preflight(tx, now)?;
            let member = group
                .member(group.state.own_leaf_index())?
                .peer()
                .fingerprint_bytes();
            let wire = group.encrypt_application_bytes(conversation::AAD, &plaintext)?;
            let reference = StableReference {
                room: group.room_id(),
                member,
                epoch: group.state.epoch().as_u64(),
                event: store::Event::OutgoingPrepared,
                wire_sha256: Sha256::digest(&wire).into(),
            };
            if enabled {
                group.capture_history(
                    tx,
                    &reference,
                    operation.body(),
                    operation.sensitive(),
                    operation.relation(),
                    now,
                )?;
            }
            Ok((wire, reference))
        })
    }

    /// Direct transport must supply its pinned peer. Legacy v1 remains ordinary
    /// text, including JSON-looking bodies; unrecognized contexts fail closed.
    pub fn decrypt_conversation_referenced(
        &mut self,
        wire: &[u8],
        sender: Option<&Peer>,
        now: u64,
    ) -> Result<(Conversation, StableReference)> {
        self.transactional_with_tx(|group, tx| {
            let enabled = group.history_preflight(tx, now)?;
            let application =
                group.decrypt_application_mut(wire, sender.map(Peer::fingerprint_bytes), true)?;
            let operation = if application.conversation {
                conversation::Operation::decode(
                    &application.plaintext,
                    group.room_id(),
                    group.state.epoch().as_u64(),
                )?
            } else {
                let (flag, bytes) = application.plaintext.split_first().ok_or(Error::Protocol)?;
                if *flag > 1 {
                    return Err(Error::Protocol);
                }
                conversation::Operation::Text(ReceivedText::from_bytes(bytes, *flag == 1)?)
            };
            let member = group
                .members
                .iter()
                .find(|(_, member)| member.peer().fingerprint() == application.sender)
                .map(|(_, member)| member.peer().fingerprint_bytes())
                .ok_or(Error::Authentication)?;
            let reference = StableReference {
                room: group.room_id(),
                member,
                epoch: group.state.epoch().as_u64(),
                event: store::Event::IncomingProcessed,
                wire_sha256: Sha256::digest(wire).into(),
            };
            if enabled {
                group.capture_history(
                    tx,
                    &reference,
                    operation.body(),
                    operation.sensitive(),
                    operation.relation(),
                    now,
                )?;
            }
            Ok((
                Conversation {
                    sender: application.sender,
                    operation,
                },
                reference,
            ))
        })
    }
}
