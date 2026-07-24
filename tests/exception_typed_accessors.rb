# Typed exception accessors: NameError#name/#receiver, NoMethodError#args,
# KeyError#key/#receiver (raised and keyword-constructed), UncaughtThrowError
# #tag/#value, SystemExit#status/#success?, and Exception#detailed_message.
begin; Object.const_get(:Nope); rescue NameError => e; p e.name; p e.receiver; end
begin; "s".no_such(1, 2); rescue NoMethodError => e; p e.name; p e.args; p e.receiver; end
begin; {"a" => 1}.fetch("z"); rescue KeyError => e; p e.key; p e.receiver; end
ke = KeyError.new("m", key: :k, receiver: {1 => 2}); p [ke.key, ke.receiver]
v = begin; throw :t, 42; rescue UncaughtThrowError => e; [e.tag, e.value]; end; p v
se = SystemExit.new(3, "done"); p [se.status, se.success?, se.message]
p SystemExit.new.success?
begin; raise "boom"; rescue => e; p e.detailed_message; end
