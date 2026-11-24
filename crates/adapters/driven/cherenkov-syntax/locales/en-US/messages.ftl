# Messages for the language profiles' errors. Each id is an error's code,
# which begins with the crate's name; the variables are its args.

cherenkov-syntax-query = { $lang }: concepts.scm: { $detail }
cherenkov-syntax-unknown-capture = { $lang }: concepts.scm: unknown capture @{ $name }
cherenkov-syntax-toml = { $lang }: { $file }: { $detail }
cherenkov-syntax-unknown-token = { $lang }: tokens.toml: unknown token "{ $name }"
cherenkov-syntax-tokens = { $lang }: tokens.toml: { $cause }
