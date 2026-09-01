# A lazy unit's `alias` of an eval-installed method validates as ITS class
# body runs, not at an earlier stream's body for the same class. rss is the
# real-world shape: 0.9 installs `pubDate` through an eval road and aliases
# `date` to it; 2.0 reopens the same classes and re-aliases. Loading must
# not raise, and both spellings answer.
require "rss/2.0"

item = RSS::Rss::Channel::Item.new
item.pubDate = Time.utc(2026, 1, 2, 3, 4, 5)
p item.date == item.pubDate
p item.date.utc.strftime("%Y-%m-%d %H:%M:%S")

channel = RSS::Rss::Channel.new
channel.pubDate = Time.utc(2026, 6, 7, 8, 9, 10)
p channel.date.utc.strftime("%Y-%m-%d %H:%M:%S")
p RSS::Rss::Channel::Item.instance_method(:date).original_name
