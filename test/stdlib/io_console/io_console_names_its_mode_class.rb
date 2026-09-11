# `require "io/console"` defines the `IO::Console` namespace and its `Mode`
# class. `IO::ConsoleMode` is the same class under its old name. None of the
# three exists before the require.
def row(label)
  r = begin; yield.inspect; rescue Exception => e; "#{e.class}: #{e.message}"; end
  puts "#{label}: #{r}"
end
row("before ConsoleMode") { defined?(IO::ConsoleMode) }
row("before Console") { defined?(IO::Console) }
row("before const_get") { IO.const_get(:ConsoleMode) }
row("before IO.constants") { IO.constants.grep(/Console/).sort }
row("before input_pending?") { $stdin.respond_to?(:input_pending?) }
require "io/console"
row("after ConsoleMode") { defined?(IO::ConsoleMode) }
row("after Console::Mode") { defined?(IO::Console::Mode) }
row("VERSION") { IO::Console::VERSION }
row("Console kind") { IO::Console.class }
row("Mode name") { IO::Console::Mode.name }
row("same class") { IO::ConsoleMode.equal?(IO::Console::Mode) }
row("ConsoleMode name") { IO::ConsoleMode.name }
row("after IO.constants") { IO.constants.grep(/Console/).sort }
row("Console.constants") { IO::Console.constants.sort }
row("Mode.constants") { IO::Console::Mode.constants }
row("Mode ancestors") { IO::Console::Mode.ancestors.first(2) }
row("Mode instance methods") { IO::Console::Mode.instance_methods(false).sort }
r, w = IO.pipe
row("empty pipe pending") { r.input_pending? }
w.write "x"
row("fed pipe pending") { r.input_pending? }
row("input_pending? arity") { IO.instance_method(:input_pending?).arity }
__END__
before ConsoleMode: nil
before Console: nil
before const_get: NameError: uninitialized constant IO::ConsoleMode
before IO.constants: []
before input_pending?: false
after ConsoleMode: "constant"
after Console::Mode: "constant"
VERSION: "0.9.2"
Console kind: Module
Mode name: "IO::Console::Mode"
same class: true
ConsoleMode name: "IO::Console::Mode"
after IO.constants: [:Console, :ConsoleMode]
Console.constants: [:Mode, :VERSION]
Mode.constants: [:VERSION]
Mode ancestors: [IO::Console::Mode, Object]
Mode instance methods: [:echo=, :raw, :raw!]
empty pipe pending: false
fed pipe pending: true
input_pending? arity: 0
