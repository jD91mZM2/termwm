# termwm

Just like Dr. Frankenstein, I too live in fear of what I have created.

\[ TODO: Place asciinema cast here \]

## What is this madness?

This is a floating WM of terminals inside your terminal.

## Use case?

If you're seriously considering using this I recommend going to a psychiatrist.

This program is so pointless, I even needed a mouse pointer to help me out find
a point.

## Running it

Make sure to pipe the output to /dev/null when running this program. This is
because the redox [ransid](https://gitlab.redox-os.org/redox-os/ransid) library
keeps spamming stdout with "Unknown CSI: ...", which messes up the view. This
is temporary until either a ransid developer stumbles across this, or I get off
my lazy ass and make a PR.

In bash:  
```bash
$ cargo run > /dev/null
```

You can also specify the shell to use:  
```bash
$ cargo run -- zsh > /dev/null
```
(It defaults to the value of `$SHELL`, or finally bash)
