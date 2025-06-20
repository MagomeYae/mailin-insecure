#!/bin/sh

swaks -tls --to fish@fish.com --pipe "cargo run -- --remote 127.0.0.1 --ssl-cert test-certs/cert.pem --ssl-key test-certs/key.pem"
