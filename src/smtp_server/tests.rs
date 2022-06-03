use lettre::{
    smtp::{ClientSecurity, SmtpClient, SmtpTransport},
    SendableEmail, Transport,
};
use lettre_email::EmailBuilder;
use tokio::runtime::Runtime;

use std::{thread, net::ToSocketAddrs};
use std::time::Duration;

use super::*;
use crate::{config::Config, email::SmtpEmail};

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
    let receiver_thread = receive_mails(1);
    thread::sleep(Duration::from_millis(100));

    // Send emails in new thread:
    let sender_thread = send_mail_local(test_email.clone().into());

    // Wait for sending thread and SMTP server to finish:
    sender_thread.join().expect("Sender thread paniced.");
    let received_mails = receiver_thread.join().expect("Receiver thread paniced.");

    // Compare received and send emails:
    assert_eq!(SmtpEmail::from(test_email), received_mails[0]);
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

fn receive_mails(n: usize) -> thread::JoinHandle<Vec<SmtpEmail>> {
    thread::spawn(move || {
        let runtime = Runtime::new().expect("Could not start Tokio runtime.");
        println!("Started Tokio runtime.");

        let mut res = vec![];
        let mut server_config = Config::default();
        server_config.local_addr = ("localhost", SMPT_TEST_PORT).to_socket_addrs().unwrap().next().unwrap();
        println!("Binding to address: {}", &server_config.local_addr);
        let smtp_server =
            runtime.block_on(SmtpServer::new(&server_config)).expect("Could not start SMTP server.");
        println!("Started SMTP server.");
        for i in 0..n {
            res.push(runtime.block_on(smtp_server.recv_mail()).expect("Could not receive email."));
            println!("Received mail {}", i);
        }

        println!("Received all mail.");
        res
    })
}
