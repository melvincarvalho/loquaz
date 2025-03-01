use druid::{im::Vector, ExtEventSink, Target};
use tokio::sync::mpsc;

use crate::{
    core::{
        config::Contact,
        conversations::ConvsNotifications,
        core::{CoreTaskHandle, CoreTaskHandleEvent},
    },
    data::{
        app_state::AppState,
        state::{
            config_state::ConfigState,
            contact_state::ContactState,
            conversation_state::{ConversationState, MessageState},
            user_state::UserState,
        },
    },
    delegate::BROKER_NOTI,
};

pub enum BrokerEvent {
    AddRelay { url: String },
    RemoveRelay { url: String },
    ConnectRelay { url: String },
    DisconnectRelay { url: String },
    AddContact { new_contact: Contact },
    RemoveContact { contact: Contact },
    SubscribeInRelays { pk: String },
    RestoreKeyPair { sk: String },
    GenerateNewKeyPair,
    SetConversation { pk: String },
    SendMessage { pk: String, content: String },
    LoadConfigs,
}

pub enum BrokerNotification {
    ConfigUpdated { config: ConfigState },
}

pub async fn start_broker(
    event_sink: ExtEventSink,
    mut broker_receiver: mpsc::Receiver<BrokerEvent>,
) {
    let mut core_handle = CoreTaskHandle::new();

    //Load configs
    send_res_ev_to_druid(
        &event_sink,
        BrokerNotification::ConfigUpdated {
            config: load_config(&core_handle),
        },
    );

    let mut rec_convs_noti = core_handle.get_convs_notifications();
    let ev_sink_clone = event_sink.clone();

    tokio::spawn(async move {
        while let Ok(noti) = rec_convs_noti.recv().await {
            match noti {
                ConvsNotifications::NewMessage(new_msg) => {
                    ev_sink_clone.add_idle_callback(move |data: &mut AppState| {
                        if data.selected_conv.is_some() {
                            let mut updated_conv = data.selected_conv.clone().unwrap();
                            updated_conv
                                .messages
                                .push_back(MessageState::from_entity(new_msg));
                            data.selected_conv = Some(updated_conv);
                        }
                    });
                }
            }
        }
    });

    while let Some(broker_event) = broker_receiver.recv().await {
        match broker_event {
            BrokerEvent::SendMessage { pk, content } => {
                core_handle.send_msg_to_contact(&pk, &content).await;
            }
            BrokerEvent::SetConversation { pk } => {
                if let Some(conv) = core_handle.get_conv(pk) {
                    event_sink.add_idle_callback(move |data: &mut AppState| {
                        data.selected_conv = Some(ConversationState::from_entity(conv));
                    });
                };
            }
            BrokerEvent::RestoreKeyPair { sk } => {
                core_handle.import_user_sk(sk);
                core_handle.subscribe().await;
                update_user_state(&event_sink, &core_handle);
            }
            BrokerEvent::GenerateNewKeyPair => {
                core_handle.gen_new_user_keypair();
                core_handle.subscribe().await;
                update_user_state(&event_sink, &core_handle);
            }
            BrokerEvent::AddRelay { url } => {
                if let CoreTaskHandleEvent::RelayAdded(Ok(_)) = core_handle.add_relay(url).await {
                    update_config_state(&event_sink, &core_handle).await;
                }
            }
            BrokerEvent::RemoveRelay { url } => {
                if let CoreTaskHandleEvent::RemovedRelay(Ok(_)) =
                    core_handle.remove_relay(url).await
                {
                    update_config_state(&event_sink, &core_handle).await;
                }
            }
            BrokerEvent::ConnectRelay { url } => {
                core_handle.connect_relay(url).await;
            }
            BrokerEvent::DisconnectRelay { url } => {
                core_handle.disconnect_relay(url).await;
            }

            BrokerEvent::SubscribeInRelays { pk } => {
                core_handle.subscribe().await;
            }
            BrokerEvent::AddContact { new_contact } => {
                let res = core_handle.add_contact(new_contact).await;
                update_config_state(&event_sink, &core_handle).await;
            }
            BrokerEvent::RemoveContact { contact } => {
                let res = core_handle.remove_contact(contact).await;
                update_config_state(&event_sink, &core_handle).await;
            }
            BrokerEvent::LoadConfigs => {
                update_config_state(&event_sink, &core_handle).await;
            }
        }
    }
}

fn load_config(core: &CoreTaskHandle) -> ConfigState {
    let (relays_url, contacts) = core.get_config();
    let mut updated_config_state = ConfigState::new();
    updated_config_state.relays_url = Vector::from(relays_url);
    updated_config_state.contacts = contacts
        .iter()
        .map(|c| ContactState::new(&c.alias, &c.pk.to_string()))
        .collect();

    updated_config_state
}

fn update_user_state(event_sink: &ExtEventSink, core_handle: &CoreTaskHandle) {
    let user = core_handle.get_user();
    event_sink.add_idle_callback(move |data: &mut AppState| {
        data.user = UserState::new(
            &user.get_sk().unwrap().to_string(),
            &user.get_pk().to_string(),
        );
    });
}

async fn update_config_state(event_sink: &ExtEventSink, core_handle: &CoreTaskHandle) {
    let (relays_url, contacts) = core_handle.get_config();
    let mut updated_config_state = ConfigState::new();
    updated_config_state.relays_url = Vector::from(relays_url);
    updated_config_state.contacts = contacts
        .iter()
        .map(|c| ContactState::new(&c.alias, &c.pk.to_string()))
        .collect();
    event_sink.add_idle_callback(move |data: &mut AppState| {
        data.config = updated_config_state;
    });
}

fn send_res_ev_to_druid(event_sink: &ExtEventSink, res: BrokerNotification) {
    event_sink
        .submit_command(BROKER_NOTI, res, Target::Auto)
        .expect("Error while send core events to Druid event sink!");
}
