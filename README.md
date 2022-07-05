# Kutsche

Kutsche is a simple SMTP server, that saves revieved emails locally or forwards them to another SMTP server.

This project is a WIP and can/should not be used in production.

## Usage

The minimal supported rust version is currently 1.61. Compile the server with

	$ cargo build --release

Run the server with

	./target/release/kutsche --config-file <path/to/config>

You can find an exemplary config file with explanations for all configuration parameters in the example directory.
