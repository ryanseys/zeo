use crate::support::run_ruby_packages;

#[test]
fn builtin_receivers_dispatch_dynamically() {
    // send_value's builtin table (oracle-verified): Array#each on a
    // literal, Hash#each, send(:length) on an Array, hash/array ops on a
    // Poly ivar, and a rescuable NoMethodError from a builtin receiver.
    let result = run_ruby_packages(
        &[(
            "main.rb",
            r##"
                [10, 20, 30].each { |x| puts x }
                { a: 1, b: 2 }.each { |k, v| puts "#{k}=#{v}" }
                puts [1, 2].send(:length)
                class Box
                  def initialize
                    @hash = {}
                    @items = []
                  end
                  def put(k, v)
                    @hash[k] = v
                    @items << k
                    self
                  end
                  def get(k)
                    @hash[k]
                  end
                  def order
                    @items
                  end
                end
                b = Box.new
                b.put(:x, 1).put(:y, 2)
                puts b.get(:y)
                puts b.order.length
                begin
                  [1, 2].no_such_method
                rescue NoMethodError
                  puts "caught NoMethodError"
                end
            "##,
        )],
        "main.rb",
        &[],
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "10\n20\n30\na=1\nb=2\n2\n2\n2\ncaught NoMethodError\n"
    );
}

/// The bundled `optparse` package parses the switch shapes it advertises,
/// with CRuby's own error messages. The full-fidelity check against real
/// `OptionParser` lives in `examples/optparse_subset.rb`.
#[test]
fn bundled_optparse_parses_switches_and_leaves_positionals() {
    let result = run_ruby_packages(
        &[(
            "main.rb",
            r##"
        require "optparse"
        opts = {}
        parser = OptionParser.new do |o|
          o.on("-v", "--verbose", "loud") { |x| opts[:verbose] = x }
          o.on("-n", "--name NAME", "who") { |v| opts[:name] = v }
        end
        argv = ["-v", "--name=matz", "file.txt", "--", "-notaflag"]
        parser.parse!(argv)
        p argv
        p opts[:verbose]
        p opts[:name]
        begin
          parser.parse!(["--nope"])
        rescue OptionParser::InvalidOption => e
          p e.message
        end
        begin
          parser.parse!(["--name"])
        rescue OptionParser::MissingArgument => e
          p e.message
        end
        "##,
        )],
        "main.rb",
        &[],
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"file.txt\", \"-notaflag\"]\ntrue\n\"matz\"\n\"invalid option: --nope\"\n\"missing argument: --name\"\n"
    );
}
