MAX_ITEMS = 5
class Config
  LIMIT = 3
  def show
    puts LIMIT
    puts MAX_ITEMS
  end
end
Config.new.show
__END__
3
5
