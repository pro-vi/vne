# Adversarial dotenv fixtures

These files are small public fixtures for parser and writer credibility. They are not examples of recommended env style; they intentionally cover cases that env editors often mishandle:

- comments, inline comments, and `#` inside values
- `export` prefixes and spacing around assignments
- quoted multiline values
- duplicate keys
- invalid or non-portable key names
- values that require quoting when edited

