# Mailin Example Program

This is an example SMTP program that uses the Mailin library.

It can be used for simple tests and as well as an inetd/xinetd service.

## Remote IP

When started by inetd/xinetd the remote IP can be read from the connection.

Though when using pipes (see below) the IP can't be read, for this reason use
`--remote <IP>` to specify which IP to be used.

(With IPs except localhost, the program checks if name and IP match.)

## Test

To test the program, either use swanks and scripts/client.sh (see below) or just call it directly:

```shell
cargo r --bin mailin-cli -- --remote 127.0.0.1 < scripts/mailsession.txt
```

## Subdirectories

The [scripts](scripts) directory contains useful scripts for playing with the server.

## Logging

The logging for this example program is done via file and stderr.

When using as an inetd/xinetd service, disable stderr logging as it will not work
(stderr is, as well as stdout, sent back). I would recommend closing stderr but rust does not allow it.

## Handler

The Handler for a Server has more bounds (namely Clone and Send) as the function to execute only one connection.

If you plan to use this for simple testing, keep in mind that when used in a server it may not compile.
Read the end of main.rs on how to enforce the bounds.
