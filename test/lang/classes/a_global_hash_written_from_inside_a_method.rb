# Assigning a key to a top-level Hash from a method body is visible outside it.
# (spinel issue #3205)
$wifis = Hash.new
class Client
  def run
    $wifis["test"] = "value"
  end
end
Client.new.run
p $wifis
__END__
{"test" => "value"}
