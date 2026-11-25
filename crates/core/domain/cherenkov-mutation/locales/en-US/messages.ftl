# Messages for the domain's errors. Each id is an error's code, which begins
# with the crate's name; the variables are its args.

cherenkov-mutation-bad-not-template = the not-template "{ $template }" must contain exactly one {"{}"}
cherenkov-mutation-duplicate-spelling = the spelling "{ $spelling }" is mapped to more than one token
cherenkov-mutation-unknown-set = no operator set named { $name }
cherenkov-mutation-unreadable-set = { $path } could not be read: { $detail }
cherenkov-mutation-invalid-set = { $path } is not an operator set: { $cause }
cherenkov-mutation-unknown-mutation = no mutation named { $name }
cherenkov-mutation-mutation-alias = { $alias } is another tool's name for { $canonical }
cherenkov-mutation-unknown-skip = no skip kind named { $name }
cherenkov-mutation-no-test-binary = no test binary
