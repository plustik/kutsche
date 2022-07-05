use lettre::{
    smtp::{ClientSecurity, SmtpClient, SmtpTransport},
    SendableEmail, Transport,
};
use lettre_email::{self, EmailBuilder};
use tokio::runtime::Runtime;

use std::time::Duration;
use std::{net::ToSocketAddrs, thread};

use super::*;
use crate::email::SmtpEmail;

const SMPT_TEST_PORT: u16 = 4025;

#[test]
fn test_mail_recv() {
    // Prepare the emails we will send and compare with the received emails:
    let test_email = EmailBuilder::new()
        .to(("test_receiver@example.org", "Firstname Lastname"))
        .from("test_sender@example.com")
        .subject("Hi, Hello world")
        .text("Hello world.")
        .build()
        .unwrap();

    // Start SMTP server:
    let mail_list = vec![test_email.clone()];
    let receiver_thread = receive_mails_cmp(mail_list);
    thread::sleep(Duration::from_millis(100));

    // Send emails in new thread:
    let sender_thread = send_mail_local(test_email.clone().into());

    // Wait for sending thread and SMTP server to finish:
    sender_thread.join().expect("Sender thread paniced.");
    let remaining_mails = receiver_thread.join().expect("Receiver thread paniced.");
    assert!(remaining_mails.is_empty());
}

fn send_mail_local(email: SendableEmail) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        // Open a local connection on port 25
        let mut mailer = SmtpTransport::new(
            SmtpClient::new(("localhost", SMPT_TEST_PORT), ClientSecurity::None).unwrap(),
        );
        // Send the email
        println!("Sending mail...");
        let result = mailer.send(email.into());

        if result.is_ok() {
            println!("Email sent");
        } else {
            println!("Could not send email: {:?}", result);
        }
    })
}

fn receive_mails_cmp(
    mut expected_mails: Vec<lettre_email::Email>,
) -> thread::JoinHandle<Vec<lettre_email::Email>> {
    thread::spawn(move || {
        let runtime = Runtime::new().expect("Could not start Tokio runtime.");
        println!("Started Tokio runtime.");

        let local_addr = ("localhost", SMPT_TEST_PORT)
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();
        println!("Binding to address: {}", local_addr);
        let smtp_server = runtime
            .block_on(SmtpServer::new(&local_addr, None))
            .expect("Could not start SMTP server.");
        println!("Started SMTP server.");
        let mut buf = vec![];
        for i in 0..expected_mails.len() {
            buf.clear();
            let (stream, addr) = runtime
                .block_on(smtp_server.accept_conn())
                .expect("Could not accept TCP connection.");
            let new_mail = runtime
                .block_on(smtp_server.recv_mail(stream, addr, &mut buf))
                .expect("Could not receive email.");
            println!("Received mail {}", i);
            rm_from_expected(&mut expected_mails, new_mail);
        }

        println!("Received all mail.");
        expected_mails
    })
}

fn rm_from_expected(expected_mails: &mut Vec<lettre_email::Email>, received_mail: SmtpEmail<'_>) {
    let mut i = 0;
    let mut found = false;
    while i < expected_mails.len() {
        // Transform SendableEmail to SmtpEmail:
        let mut buf = vec![];
        let tokio_mail = expected_mails[i].clone().into();
        let smpt_email = SmtpEmail::from_tokio_mail(tokio_mail, &mut buf);
        if smpt_email == received_mail {
            expected_mails.remove(i);
            found = true;
            break;
        }
        i += 1;
    }
    assert!(found, "Received an unexpected email.");
}
