//! POSIX shell parser: pipelines, lists (`&&`, `||`, `;`), command
//! substitution, quoting. Built so that `ls && rm -rf /` is classified by
//! its worst member and quote-concatenation evasions do not hide a verb.
