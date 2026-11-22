# Security policy

Hidely writes a small piece of code into other processes to do its job, so its security matters.
Reports are very welcome.

## Supported versions

Only the [latest release](https://github.com/CodingRule/Hidely/releases/latest) receives fixes.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately instead:

1. Go to the repository's **Security** tab.
2. Choose **Report a vulnerability**.
3. Describe what you found, how to reproduce it, and what an attacker could do with it.

You should get a reply within a week. Once a fix is released you will be credited in the release
notes, unless you prefer not to be.

## What counts

- Hidely leaving a window hidden when it should have been restored
- Another program being able to misuse Hidely to run code or gain privileges
- The installer or release files being tampered with
- Anything in the test page that sends the captured screen somewhere

## What doesn't

- Antivirus products flagging Hidely. Writing into another process looks like malware behaviour
  to them, and that is expected. If a specific product blocks it, a normal issue is fine.
- The files in `driver/`. They are an unmodified proof of concept from DWMShield that Hidely
  does not build or ship. Known problems are listed in [docs/DRIVER.md](docs/DRIVER.md).
