//! Bounded Send handle; private Group never leaves its owning local task/thread.
use super::*;
use crate::{
    pairing::RoomPeer,
    rooms::{Rights, control::raft::Node, enrollment::Authority},
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::Instant,
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Status {
    pub owner: crate::rooms::control::raft::PeerId,
    pub room: [u8; 32],
    pub epoch: u64,
    pub applied: Option<Position>,
    pub active: bool,
    pub members: Vec<super::super::control::raft::PeerId>,
    pub running: bool,
    pub busy: bool,
    pub history: history::Summary,
}
impl Status {
    fn snapshot(room: &Room, running: bool, busy: bool) -> Self {
        Self {
            owner: crate::rooms::control::raft::PeerId::from_peer(&room.identity.peer()),
            room: room.room_id(),
            epoch: room.epoch(),
            applied: room.applied_position(),
            active: room.is_active(),
            members: room
                .peers()
                .iter()
                .map(crate::rooms::control::raft::PeerId::from_peer)
                .collect(),
            running,
            busy,
            history: room.history_summary.clone(),
        }
    }
}
/// Plaintext is memory-only and uses ReceivedText's sensitive-output policy.
/// No Debug implementation or automatic history writer is installed here.
pub enum Event {
    /// Legacy producer payload. Authenticated actor receives use ReferencedText.
    Text(Text),
    ReferencedText {
        text: Text,
        reference: crate::rooms::group::StableReference,
    },
    Conversation {
        conversation: crate::rooms::group::Conversation,
        reference: crate::rooms::group::StableReference,
    },
}
impl Event {
    fn conversation(
        conversation: crate::rooms::group::Conversation,
        reference: crate::rooms::group::StableReference,
    ) -> Self {
        match conversation.operation {
            crate::rooms::conversation::Operation::Text(content) => Self::ReferencedText {
                text: Text {
                    sender: conversation.sender,
                    content,
                },
                reference,
            },
            operation => Self::Conversation {
                conversation: crate::rooms::group::Conversation {
                    sender: conversation.sender,
                    operation,
                },
                reference,
            },
        }
    }
}

/// Ciphertext and its exact epoch audience, captured in one actor operation.
pub struct Publication {
    pub wire: Vec<u8>,
    pub members: Vec<crate::rooms::control::raft::PeerId>,
}
pub struct ReferencedPublication {
    pub publication: Publication,
    pub reference: crate::rooms::group::StableReference,
}

const HISTORY_QUEUE: usize = 2;

enum Command {
    EncryptConversation {
        operation: crate::rooms::conversation::Operation,
        audience: crate::rooms::conversation::Audience,
        reply: oneshot::Sender<Result<ReferencedPublication>>,
    },
    History {
        operation: history::Operation,
        reply: oneshot::Sender<Result<history::Summary>>,
    },
    HistorySearch {
        search: history::Search,
        reply: oneshot::Sender<Result<history::Page>>,
    },
    EncryptReferenced {
        text: Zeroizing<String>,
        sensitive: bool,
        reply: oneshot::Sender<Result<ReferencedPublication>>,
    },
    AcceptReferenced {
        peer: Peer,
        wire: Vec<u8>,
        reply: oneshot::Sender<Result<crate::rooms::group::StableReference>>,
    },
    MediaPolicy(oneshot::Sender<Result<MediaPolicy>>),
    Synchronize(oneshot::Sender<Result<()>>),
    Encrypt {
        text: Zeroizing<String>,
        sensitive: bool,
        reply: oneshot::Sender<Result<Publication>>,
    },
    Accept {
        peer: Peer,
        wire: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    FileAuthority {
        peer: Peer,
        inbound: bool,
        reply: oneshot::Sender<Result<()>>,
    },
    ProfileMetadataAuthority {
        peer: Peer,
        reply: oneshot::Sender<Result<u64>>,
    },
    Admit {
        package: Vec<u8>,
        reply: oneshot::Sender<Result<Proposal>>,
    },
    ScopedAdmit {
        approval: Box<RoomPeer>,
        package: Vec<u8>,
        reply: oneshot::Sender<Result<Proposal>>,
    },
    Authority {
        approval: Box<RoomPeer>,
        rights: Rights,
        reply: oneshot::Sender<Result<Authority>>,
    },
    Learner {
        approval: Box<RoomPeer>,
        node: Node,
        reply: oneshot::Sender<Result<()>>,
    },
    Controllers {
        voters: std::collections::BTreeSet<crate::rooms::control::raft::PeerId>,
        reply: oneshot::Sender<Result<openraft::LogId<crate::rooms::control::raft::PeerId>>>,
    },
    Remove {
        peer: Peer,
        reply: oneshot::Sender<Result<Proposal>>,
    },
    Submit(oneshot::Sender<Result<ApplyResult>>),
    Welcome {
        commit: Vec<u8>,
        reply: oneshot::Sender<Result<Option<Vec<u8>>>>,
    },
    Acknowledge {
        commit: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    Shutdown(oneshot::Sender<Result<()>>),
}
impl Command {
    fn cancelled(&self) -> bool {
        match self {
            Self::History { reply, .. } => reply.is_closed(),
            Self::HistorySearch { reply, .. } => reply.is_closed(),
            Self::EncryptReferenced { reply, .. } => reply.is_closed(),
            Self::EncryptConversation { reply, .. } => reply.is_closed(),
            Self::AcceptReferenced { reply, .. } => reply.is_closed(),
            Self::MediaPolicy(reply) => reply.is_closed(),
            Self::Synchronize(reply) | Self::Shutdown(reply) => reply.is_closed(),
            Self::Encrypt { reply, .. } => reply.is_closed(),
            Self::Accept { reply, .. } | Self::Acknowledge { reply, .. } => reply.is_closed(),
            Self::FileAuthority { reply, .. } => reply.is_closed(),
            Self::ProfileMetadataAuthority { reply, .. } => reply.is_closed(),
            Self::Admit { reply, .. }
            | Self::Remove { reply, .. }
            | Self::ScopedAdmit { reply, .. } => reply.is_closed(),
            Self::Authority { reply, .. } => reply.is_closed(),
            Self::Learner { reply, .. } => reply.is_closed(),
            Self::Controllers { reply, .. } => reply.is_closed(),
            Self::Submit(reply) => reply.is_closed(),
            Self::Welcome { reply, .. } => reply.is_closed(),
        }
    }
}
struct Request {
    expires: Instant,
    command: Command,
    cancelled: oneshot::Receiver<()>,
}
struct CancelOnDrop(Option<oneshot::Sender<()>>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
/// Bounded actor commands. Cancellation/timeout after a private commit has an
/// uncertain outcome: it withholds the result, not a rollback of durable state.
/// Refresh authorized status before explicitly retrying a mutation. Priority
/// control admission cannot preempt active synchronous crypto/filesystem work.
#[derive(Clone)]
pub struct Handle {
    sender: mpsc::Sender<Request>,
    history_sender: mpsc::Sender<Request>,
    status: watch::Receiver<Status>,
}
pub struct Actor {
    room: Room,
    requests: mpsc::Receiver<Request>,
    history_requests: mpsc::Receiver<Request>,
    events: mpsc::Sender<Event>,
    status: watch::Sender<Status>,
}

impl Handle {
    /// Cached public summary only. Use history_refresh for an authorized fresh read.
    pub async fn history_status(&self, deadline: Duration) -> Result<history::Summary> {
        Ok(self.status(deadline).await?.history)
    }
    pub async fn history_enable_file(
        &self,
        policy: history::Policy,
        deadline: Duration,
    ) -> Result<history::Summary> {
        policy.validate()?;
        self.history_operation(history::Operation::Enable(policy), deadline)
            .await
    }
    pub async fn history_open_file(&self, deadline: Duration) -> Result<history::Summary> {
        self.history_operation(history::Operation::Open, deadline)
            .await
    }
    pub async fn history_recover_file(
        &self,
        expected: age::x25519::Recipient,
        policy: history::Policy,
        deadline: Duration,
    ) -> Result<history::Summary> {
        policy.validate()?;
        self.history_operation(history::Operation::Recover(expected, policy), deadline)
            .await
    }
    /// Typed SDK key, never Debug/Serialize or an HTTP string parameter.
    pub async fn history_use_identity(
        &self,
        identity: age::x25519::Identity,
        deadline: Duration,
    ) -> Result<history::Summary> {
        self.history_operation(history::Operation::Identity(identity), deadline)
            .await
    }
    pub async fn history_enable(
        &self,
        policy: history::Policy,
        deadline: Duration,
    ) -> Result<history::Summary> {
        policy.validate()?;
        self.history_operation(history::Operation::EnableIdentity(policy), deadline)
            .await
    }
    pub async fn history_disable(&self, deadline: Duration) -> Result<history::Summary> {
        self.history_operation(history::Operation::Disable, deadline)
            .await
    }
    pub async fn history_clear(&self, deadline: Duration) -> Result<history::Summary> {
        self.history_operation(history::Operation::Clear, deadline)
            .await
    }
    pub async fn history_refresh(&self, deadline: Duration) -> Result<history::Summary> {
        self.history_operation(history::Operation::Status, deadline)
            .await
    }
    async fn history_operation(
        &self,
        operation: history::Operation,
        deadline: Duration,
    ) -> Result<history::Summary> {
        self.call(deadline, |reply| Command::History { operation, reply })
            .await
    }
    pub async fn history_search(
        &self,
        search: history::Search,
        deadline: Duration,
    ) -> Result<history::Page> {
        search.validate()?;
        self.call(deadline, |reply| Command::HistorySearch { search, reply })
            .await
    }
    pub async fn publication_referenced(
        &self,
        text: String,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<ReferencedPublication> {
        let text = Zeroizing::new(text);
        if text.len() > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        self.call(deadline, |reply| Command::EncryptReferenced {
            text,
            sensitive,
            reply,
        })
        .await
    }
    pub async fn publication_conversation(
        &self,
        operation: crate::rooms::conversation::Operation,
        audience: crate::rooms::conversation::Audience,
        deadline: Duration,
    ) -> Result<ReferencedPublication> {
        self.call(deadline, |reply| Command::EncryptConversation {
            operation,
            audience,
            reply,
        })
        .await
    }
    pub async fn accept_text_referenced(
        &self,
        peer: Peer,
        wire: Vec<u8>,
        deadline: Duration,
    ) -> Result<crate::rooms::group::StableReference> {
        bound(&wire)?;
        self.call(deadline, |reply| Command::AcceptReferenced {
            peer,
            wire,
            reply,
        })
        .await
    }
    pub async fn media_policy(&self, deadline: Duration) -> Result<MediaPolicy> {
        self.call(deadline, Command::MediaPolicy).await
    }
    async fn call<T>(
        &self,
        deadline: Duration,
        build: impl FnOnce(oneshot::Sender<Result<T>>) -> Command,
    ) -> Result<T> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        let (reply, received) = oneshot::channel();
        // Dropping/cancelling the caller stops work at its next async boundary.
        // It does not undo an already committed operation: that stays uncertain.
        let (cancel, cancelled) = oneshot::channel();
        let _cancel = CancelOnDrop(Some(cancel));
        let command = build(reply);
        let history_work = matches!(
            &command,
            Command::History { .. } | Command::HistorySearch { .. }
        );
        let sender = if history_work {
            &self.history_sender
        } else {
            &self.sender
        };
        sender
            .try_send(Request {
                expires: Instant::now() + deadline,
                command,
                cancelled,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => Error::Limit {
                    resource: "room command queue",
                    max: if history_work {
                        HISTORY_QUEUE
                    } else {
                        config::ROOM_COMMAND_QUEUE
                    },
                },
                mpsc::error::TrySendError::Closed(_) => Error::Cancelled,
            })?;
        tokio::time::timeout(deadline, received)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Cancelled)?
    }
    /// Cached DURABLE local status only; not a freshness/quorum claim.
    pub async fn status(&self, deadline: Duration) -> Result<Status> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        if self.sender.is_closed() {
            return Err(Error::Cancelled);
        }
        Ok(self.status.borrow().clone())
    }
    pub(crate) async fn ready(&self, deadline: Duration) -> Result<Status> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        let mut status = self.status.clone();
        let ready = tokio::time::timeout(deadline, async {
            status
                .wait_for(|status| status.running)
                .await
                .map(|status| status.clone())
                .map_err(|_| Error::Cancelled)
        })
        .await
        .map_err(|_| Error::Timeout)??;
        if self.sender.is_closed() {
            return Err(Error::Cancelled);
        }
        Ok(ready)
    }
    pub async fn synchronize(&self, deadline: Duration) -> Result<()> {
        self.call(deadline, Command::Synchronize).await
    }
    pub async fn encrypt_text(
        &self,
        text: String,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        Ok(self.publication(text, sensitive, deadline).await?.wire)
    }
    pub async fn publication(
        &self,
        text: String,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<Publication> {
        let text = Zeroizing::new(text);
        if text.len() > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        self.call(deadline, |reply| Command::Encrypt {
            text,
            sensitive,
            reply,
        })
        .await
    }
    /// ACK here means durable replay processing plus the bounded application
    /// event queue, not human reading or plaintext persistence.
    pub async fn accept_text(&self, peer: Peer, wire: Vec<u8>, deadline: Duration) -> Result<()> {
        bound(&wire)?;
        self.call(deadline, |reply| Command::Accept { peer, wire, reply })
            .await
    }
    pub async fn authorize_file(
        &self,
        peer: Peer,
        inbound: bool,
        deadline: Duration,
    ) -> Result<()> {
        self.call(deadline, |reply| Command::FileAuthority {
            peer,
            inbound,
            reply,
        })
        .await
    }
    /// Quorum-current metadata authorization. The returned epoch is a snapshot;
    /// callers must reauthorize at disclosure and response acceptance boundaries.
    pub async fn authorize_profile_metadata(&self, peer: Peer, timeout: Duration) -> Result<u64> {
        self.call(timeout, |reply| Command::ProfileMetadataAuthority {
            peer,
            reply,
        })
        .await
    }
    pub async fn prepare_admission(
        &self,
        package: Vec<u8>,
        deadline: Duration,
    ) -> Result<Proposal> {
        bound(&package)?;
        self.call(deadline, |reply| Command::Admit { package, reply })
            .await
    }
    pub async fn prepare_scoped_admission(
        &self,
        approval: RoomPeer,
        package: Vec<u8>,
        deadline: Duration,
    ) -> Result<Proposal> {
        bound(&package)?;
        scope_bound(&approval)?;
        self.call(deadline, |reply| Command::ScopedAdmit {
            approval: Box::new(approval),
            package,
            reply,
        })
        .await
    }
    pub async fn issue_enrollment_authority(
        &self,
        approval: RoomPeer,
        rights: Rights,
        deadline: Duration,
    ) -> Result<Authority> {
        scope_bound(&approval)?;
        self.call(deadline, |reply| Command::Authority {
            approval: Box::new(approval),
            rights,
            reply,
        })
        .await
    }
    pub async fn provision_scoped_learner(
        &self,
        approval: RoomPeer,
        node: Node,
        deadline: Duration,
    ) -> Result<()> {
        scope_bound(&approval)?;
        self.call(deadline, |reply| Command::Learner {
            approval: Box::new(approval),
            node,
            reply,
        })
        .await
    }
    pub async fn approve_controllers(
        &self,
        voters: std::collections::BTreeSet<crate::rooms::control::raft::PeerId>,
        deadline: Duration,
    ) -> Result<openraft::LogId<crate::rooms::control::raft::PeerId>> {
        if voters.is_empty() || voters.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Configuration(
                "controller selection must be nonempty and bounded",
            ));
        }
        self.call(deadline, |reply| Command::Controllers { voters, reply })
            .await
    }
    pub async fn prepare_departure(&self, peer: Peer, deadline: Duration) -> Result<Proposal> {
        self.call(deadline, |reply| Command::Remove { peer, reply })
            .await
    }
    pub async fn submit_prepared(&self, deadline: Duration) -> Result<ApplyResult> {
        self.call(deadline, Command::Submit).await
    }
    pub async fn committed_welcome(
        &self,
        commit: Vec<u8>,
        deadline: Duration,
    ) -> Result<Option<Vec<u8>>> {
        bound(&commit)?;
        self.call(deadline, |reply| Command::Welcome { commit, reply })
            .await
    }
    pub async fn acknowledge_welcome(&self, commit: Vec<u8>, deadline: Duration) -> Result<()> {
        bound(&commit)?;
        self.call(deadline, |reply| Command::Acknowledge { commit, reply })
            .await
    }
    pub async fn shutdown(&self, deadline: Duration) -> Result<()> {
        self.call(deadline, Command::Shutdown).await
    }
}
fn bound(bytes: &[u8]) -> Result<()> {
    if bytes.len() > config::ROOM_MAX_WIRE_BYTES {
        return Err(Error::Limit {
            resource: "room actor input",
            max: config::ROOM_MAX_WIRE_BYTES,
        });
    }
    Ok(())
}
fn scope_bound(approval: &RoomPeer) -> Result<()> {
    if approval.local_identity.len() != 64 {
        return Err(Error::Authentication);
    }
    Ok(())
}

impl Actor {
    /// Does not spawn. The caller explicitly runs Actor::run on a LocalSet or
    /// owning foreground thread and retains the join handle for orderly shutdown.
    pub fn new(room: Room) -> (Handle, Self, mpsc::Receiver<Event>) {
        let (sender, requests) = mpsc::channel(config::ROOM_COMMAND_QUEUE);
        let (history_sender, history_requests) = mpsc::channel(HISTORY_QUEUE);
        let (events, received) = mpsc::channel(config::ROOM_EVENT_QUEUE);
        let (status, observed) = watch::channel(Status::snapshot(&room, false, false));
        (
            Handle {
                sender,
                history_sender,
                status: observed,
            },
            Self {
                room,
                requests,
                history_requests,
                events,
                status,
            },
            received,
        )
    }
    pub async fn run(mut self) -> Room {
        self.publish(true, false);
        loop {
            let mut request = tokio::select! {
                biased;
                request = self.requests.recv() => match request { Some(request) => request, None => break },
                request = self.history_requests.recv() => match request { Some(request) => request, None => break },
            };
            if request.expires <= Instant::now() || request.command.cancelled() {
                continue;
            }
            self.publish(true, true);
            let shutdown = tokio::select! {
                biased;
                _ = &mut request.cancelled => false,
                shutdown = tokio::time::timeout_at(request.expires, self.execute(request.command, request.expires)) => shutdown.unwrap_or(false),
            };
            self.publish(true, false);
            if shutdown {
                break;
            }
        }
        self.publish(false, false);
        self.room
    }
    fn publish(&self, running: bool, busy: bool) {
        self.status
            .send_replace(Status::snapshot(&self.room, running, busy));
    }
    async fn execute(&mut self, command: Command, expires: Instant) -> bool {
        let deadline = expires.saturating_duration_since(Instant::now());
        match command {
            Command::EncryptConversation {
                operation,
                audience,
                reply,
            } => {
                let result = self
                    .room
                    .encrypt_conversation_referenced(&operation, &audience, deadline)
                    .await
                    .map(|(wire, reference)| ReferencedPublication {
                        publication: Publication {
                            wire,
                            members: self
                                .room
                                .peers()
                                .iter()
                                .map(crate::rooms::control::raft::PeerId::from_peer)
                                .collect(),
                        },
                        reference,
                    });
                if !reply.is_closed() {
                    let _ = reply.send(before_deadline(result, expires));
                }
            }
            Command::History { operation, reply } => {
                let result = self.room.history_operation(operation, deadline).await;
                let _ = reply.send(before_deadline(result, expires));
            }
            Command::HistorySearch { search, reply } => {
                let result = self.room.history_search(search, deadline).await;
                let _ = reply.send(before_deadline(result, expires));
            }
            Command::EncryptReferenced {
                text,
                sensitive,
                reply,
            } => {
                let result = self
                    .room
                    .encrypt_text_referenced(&text, sensitive, deadline)
                    .await
                    .map(|(wire, reference)| ReferencedPublication {
                        publication: Publication {
                            wire,
                            members: self
                                .room
                                .peers()
                                .iter()
                                .map(crate::rooms::control::raft::PeerId::from_peer)
                                .collect(),
                        },
                        reference,
                    });
                if !reply.is_closed() {
                    let _ = reply.send(before_deadline(result, expires));
                }
            }
            Command::AcceptReferenced { peer, wire, reply } => {
                let result = match self.events.try_reserve() {
                    Ok(permit) => match self
                        .room
                        .decrypt_conversation_referenced(&peer, &wire, deadline)
                        .await
                    {
                        Ok((conversation, reference))
                            if !reply.is_closed() && Instant::now() < expires =>
                        {
                            permit.send(Event::conversation(conversation, reference.clone()));
                            Ok(reference)
                        }
                        Ok(_) => Err(Error::Cancelled),
                        Err(error) => Err(error),
                    },
                    Err(mpsc::error::TrySendError::Full(_)) => Err(Error::Limit {
                        resource: "room event queue",
                        max: config::ROOM_EVENT_QUEUE,
                    }),
                    Err(mpsc::error::TrySendError::Closed(_)) => Err(Error::Cancelled),
                };
                let _ = reply.send(before_deadline(result, expires));
            }
            Command::MediaPolicy(reply) => {
                let _ = reply.send(self.room.media_policy(deadline).await);
            }
            Command::Synchronize(reply) => {
                let _ = reply.send(self.room.synchronize(deadline).await.map(|_| ()));
            }
            Command::Encrypt {
                text,
                sensitive,
                reply,
            } => {
                let result = self
                    .room
                    .encrypt_text(&text, sensitive, deadline)
                    .await
                    .map(|wire| Publication {
                        wire,
                        members: self
                            .room
                            .peers()
                            .iter()
                            .map(crate::rooms::control::raft::PeerId::from_peer)
                            .collect(),
                    });
                let _ = reply.send(before_deadline(result, expires));
            }
            Command::Accept { peer, wire, reply } => {
                // Reserve before decrypting; a full/closed UI cannot consume
                // a ratchet and then silently discard the resulting event.
                let result = match self.events.try_reserve() {
                    Ok(permit) => match self
                        .room
                        .decrypt_conversation_referenced(&peer, &wire, deadline)
                        .await
                    {
                        Ok((conversation, reference))
                            if !reply.is_closed() && Instant::now() < expires =>
                        {
                            permit.send(Event::conversation(conversation, reference));
                            Ok(())
                        }
                        Ok(_) => Err(Error::Cancelled),
                        Err(error) => Err(error),
                    },
                    Err(mpsc::error::TrySendError::Full(_)) => Err(Error::Limit {
                        resource: "room event queue",
                        max: config::ROOM_EVENT_QUEUE,
                    }),
                    Err(mpsc::error::TrySendError::Closed(_)) => Err(Error::Cancelled),
                };
                let _ = reply.send(result);
            }
            Command::FileAuthority {
                peer,
                inbound,
                reply,
            } => {
                let _ = reply.send(self.room.authorize_file(&peer, inbound, deadline).await);
            }
            Command::ProfileMetadataAuthority { peer, reply } => {
                let _ = reply.send(self.room.authorize_profile_metadata(&peer, deadline).await);
            }
            Command::Admit { package, reply } => {
                let _ = reply.send(self.room.prepare_admission(&package, deadline).await);
            }
            Command::ScopedAdmit {
                approval,
                package,
                reply,
            } => {
                let _ = reply.send(
                    self.room
                        .prepare_scoped_admission(&approval, &package, deadline)
                        .await,
                );
            }
            Command::Authority {
                approval,
                rights,
                reply,
            } => {
                let _ = reply.send(
                    self.room
                        .issue_enrollment_authority(&approval, rights, deadline)
                        .await,
                );
            }
            Command::Learner {
                approval,
                node,
                reply,
            } => {
                let _ = reply.send(
                    self.room
                        .provision_scoped_learner(&approval, node, deadline)
                        .await,
                );
            }
            Command::Controllers { voters, reply } => {
                let _ = reply.send(self.room.approve_controllers(voters, deadline).await);
            }
            Command::Remove { peer, reply } => {
                let _ = reply.send(self.room.prepare_departure(&peer, deadline).await);
            }
            Command::Submit(reply) => {
                let _ = reply.send(self.room.submit_prepared(deadline).await);
            }
            Command::Welcome { commit, reply } => {
                let _ = reply.send(self.room.committed_welcome(&commit));
            }
            Command::Acknowledge { commit, reply } => {
                let _ = reply.send(self.room.acknowledge_welcome(&commit));
            }
            Command::Shutdown(reply) => {
                let _ = reply.send(Ok(()));
                return true;
            }
        }
        false
    }
}

#[cfg(all(test, unix))]
mod tests;

fn before_deadline<T>(result: Result<T>, expires: Instant) -> Result<T> {
    if Instant::now() >= expires {
        Err(Error::Timeout)
    } else {
        result
    }
}

#[cfg(all(test, unix))]
#[path = "history_tests.rs"]
mod history_tests;
