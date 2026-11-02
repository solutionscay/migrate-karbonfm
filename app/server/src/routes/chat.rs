//! UI-21 Messages: a static mock of SMS chat (OD-05-02: SMS is not migrated and not built in this version). The
//! conversations are hard-coded samples taken from design/messages.html. The page reads no chat data, writes nothing
//! and posts nothing: the message box and the send button are disabled. The database is read only for the sidebar's
//! DEMO badge, as on every page.

use super::Nav;
use crate::web::{render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::State;
use axum::response::Html;

/// One conversation of the list.
pub struct Conversation {
    pub initials: &'static str,
    pub color: &'static str,
    pub name: &'static str,
    pub when: &'static str,
    pub last: &'static str,
    pub unread: bool,
}

/// One row of the open conversation: a day line, a system line or a message.
pub enum Line {
    Day(&'static str),
    System { text: &'static str, at: &'static str },
    Message { outgoing: bool, text: &'static str, meta: Option<&'static str> },
}

pub const CONVERSATIONS: &[Conversation] = &[
    Conversation {
        initials: "EH",
        color: "c4",
        name: "Elena Halden",
        when: "9:05",
        last: "You: Absolutely. I’ll have a render to…",
        unread: false,
    },
    Conversation {
        initials: "TF",
        color: "c3",
        name: "Tomás Ferreira",
        when: "8:20",
        last: "Perfect, see you Thursday at 10",
        unread: true,
    },
    Conversation {
        initials: "DW",
        color: "c7",
        name: "Dana Whitlock",
        when: "Yest.",
        last: "Check goes out Friday, promise!",
        unread: true,
    },
    Conversation {
        initials: "OC",
        color: "c2",
        name: "Owen Castellano",
        when: "Yest.",
        last: "Can we push the site visit to 2pm?",
        unread: true,
    },
    Conversation {
        initials: "LM",
        color: "c5",
        name: "Lena Moreau",
        when: "Mon",
        last: "You: Proof attached. Let me know!",
        unread: false,
    },
    Conversation {
        initials: "IT",
        color: "c1",
        name: "Ivy Tran",
        when: "Oct 22",
        last: "Thanks, received 👍",
        unread: false,
    },
    Conversation {
        initials: "NO",
        color: "c4",
        name: "Dr. Ngozi Okafor",
        when: "Oct 19",
        last: "You: Renewal reminder for your support…",
        unread: false,
    },
];

/// The open conversation (Elena Halden). A time shows under the last message of a run, as in the design.
pub const LINES: &[Line] = &[
    Line::Day("Tue, Oct 27"),
    Line::Message { outgoing: false, text: "Hi Mara, it’s Elena. Quick one: did the A1 boards go to print yet?", meta: Some("4:02 PM") },
    Line::Message { outgoing: true, text: "Hi Elena! Kai picked them up this morning. They’ll be at your office by 10 tomorrow.", meta: Some("4:09 PM") },
    Line::Message { outgoing: false, text: "Amazing, thank you 🙌", meta: Some("4:10 PM") },
    Line::Day("Today"),
    Line::System { text: "Payment of $1,000.00 recorded on INV-1048", at: "8:31 AM" },
    Line::Message { outgoing: false, text: "Boards look great. Marcus loves the oak detail.", meta: None },
    Line::Message {
        outgoing: false,
        text: "One thing: I left a comment on the lighting thread. Could we see the 2700K globes in a render before Friday?",
        meta: Some("8:47 AM"),
    },
    Line::Message {
        outgoing: true,
        text: "Absolutely. I’ll have a render to you by Thursday afternoon, and I’ll fold the revised quote in with it.",
        meta: Some("9:05 AM · Delivered"),
    },
];

/// The quick replies of the design, shown disabled.
pub const QUICK_REPLIES: &[&str] = &["Payment reminder", "Confirm visit", "Thanks"];

#[derive(Template)]
#[template(path = "chat.html")]
struct ChatPage {
    nav: Nav,
    demo: bool,
    conversations: &'static [Conversation],
    lines: &'static [Line],
    quick_replies: &'static [&'static str],
}

/// `GET /chat` (UI-21).
pub async fn page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let demo = super::demo(&state)?;
    render(&ChatPage {
        nav: Nav::Messages,
        demo,
        conversations: CONVERSATIONS,
        lines: LINES,
        quick_replies: QUICK_REPLIES,
    })
}
