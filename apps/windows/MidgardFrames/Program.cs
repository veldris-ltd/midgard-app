using MidgardFrames;

// midgard-frames.exe: see Cli.cs. Frames go straight to the process's stdout handle (binary, no
// console encoding); messages go to stderr.
using var stdout = Console.OpenStandardOutput();
return Cli.Run(args, stdout, Console.Error);
