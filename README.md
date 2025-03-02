A tool for bisecting cdda releases. Same idea as `git bisect`, but for... not git.

Not remotely production ready, and no intent to make it such. It's just a personal tool, written as a one-off initially. Use at your own risk yada yada. 

### Installation 

1) install rust - https://www.rust-lang.org/tools/install
2) copy `config.example.json5` to `config.json5` and adjust the paths accordingly. You will need 7-zip.
3) build and run:
```ps1
> cargo run
```

### Usage

Launch with `cargo run` (you can also find an executable in `./target/debug/` folder after you run either `cargo run` or `cargo build`)

You would be dropped into a primitive shell. Your options:

* `reset` - clear the current bisection state and start anew
* `next` - move to a next candidate version to try. Or to the first version to try if you haven't tried any yet. Has special forms
  * `next` - see above
  * `next <number>d` - e.g. `next 90d` - move to a version this many days prior to the activated one.
* `activate` - sets a specific cdda version as "active".  Has several forms:
  * `activate <tag-name>` - for example `activate cdda-experimental-2025-03-02-0012`
  * `activate tip` - activate the absolute freshest release that exists on github
  * `activate recent` - activate the most recent release that is *downloaded*
* `run` - launch the currently selected version of the game
* `mark` - marks the currently selected version of the game as either good or bad
  * `mark good` - "this is an earlier version, without the bug yet"
  * `mark bad`  - "this is a newer version with the bug already"
  * `mark blacklist` - "this is turbo broken and does not even start, do not suggest me this version ever again". Not well tested.
  * `mark skip` - "ignore this version for the current session, but it might be fine in the future". Known buggy.
* `track` - show which versions we've marked as what so far
* `fix-font` - deletes `fonts.json` from cdda config directory to work around a recent backwards-incomaptible change in the parsing of that file.

#### Typical workflow:

```ps1
> reset     # start a new session
> next      # grab a release
> run       # run it
> mark bad  # buggy
> next      # grab next
> run       # run it again
> mark bad  # still buggy
> next 90d  # let's try something ancient
> run       # it's annoying to constantly invoke `run` explicitly, but it is what it is
> mark good # ok, this one works
> next
> run
> mark good # still works
> next
> run
> mark bad # this one doesn't
> next
> run
> mark good 
.....
> run
> mark good
> next
Bisected to commit range ( a473f3c3ecde1901d7a3e8db0f7810034e89668a , d59a5d6ab8e7b692d584c254dca60e561aef5176 ]
  latest good - [cdda-experimental-2025-02-08-1735](https://github.com/CleverRaven/Cataclysm-DDA/releases/tag/cdda-experimental-2025-02-08-1735)
  earliest bad - [cdda-experimental-2025-02-08-1934](https://github.com/CleverRaven/Cataclysm-DDA/releases/tag/cdda-experimental-2025-02-08-1934)
```
yay
