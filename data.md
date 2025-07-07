# A short description about these commits

## Remove dyn handler

Use generics instead of dyn.

The size gain for mailin-server is minimal, the performance gain also probably.

Downside: when using multiple Handlers in one project the size will increase.

Is a reqiorement for `Rework data*, add Filter`

## Remove unused bound

It's just not used.

## Use write_all in test

To simplify and streamline things

## Add missing end of code

As it says

## Change Handler::data* to return Result<(), Response>

This and the following two were one big commit, I've tried my best to put it in smaller chunks...

As it says, moves the error! code around.

If a `Err(Response)` is returned and the `Response` is not an error then it's replaced by `INTERNAL_ERROR`.

## Rework data*, add Filter

This is the mail idea.

Create a trait for data handling.

The data_start does return a state, which is passes (as reference) into data and (consuming) in data_end.

This should make it much easier to handle the state of a data write.

Also store does not hold a mime-event parser anymore.

The mime-event parser is "stand alone".

There are impls for (A,B) and (A,B,C) for Data to combine multiple Data's (pass all data into everyone).
That way they don't need to be stacked anymore.

(I can write a macro which also declares this impl for up to X tuples - but how many are needed)

## mime-event: Response instead of io::Error

This only moves where the Response/io:Error is used as it's Response always externally.
