# Don't split what fits in one line

If something can be written in one line, do not split it across functions, structs, files, or extra types unless the user specifically requests that split.

# No thin wrappers

A type or function whose only job is to wrap another type or call is unacceptable. If a refactor leaves one behind, delete it and use the inner type or call directly.

# No comments

Do not add comments. Delete comments that are not absolutely, undeniably essential. Clap `help` attributes are allowed. `//` and `///` on code are not.
