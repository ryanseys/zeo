$wifis = Hash.new
class Client
  def run
    $wifis["test"] = "value"
  end
end
Client.new.run
p $wifis
