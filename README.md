# Kutsche

Kutsche is a simple SMTP server, that saves revieved emails locally or forwards them to another SMTP server.

This project is a WIP and can/should not be used in production.

## Usage

Compile the server with

	$ cargo build --release

Run the server with

	./target/release/kutsche --config-file <path/to/config>

You can find an exemplary config file in the example directory.
