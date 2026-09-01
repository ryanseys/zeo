# The full rss gem loads and parses: its 0.9/2.0 units cross-alias
# eval-installed accessors (the per-body alias check), dublincore defines
# element classes in module_eval strings (the caller-cref rule), and
# Element.inherited reads the eval-defined class's name.
require "rss"

xml = <<~XML
  <?xml version="1.0"?>
  <rss version="2.0">
    <channel>
      <title>zeo news</title>
      <link>http://example.com/</link>
      <description>updates</description>
      <pubDate>Tue, 01 Sep 2026 12:00:00 +0000</pubDate>
      <item>
        <title>first post</title>
        <link>http://example.com/1</link>
        <pubDate>Mon, 31 Aug 2026 09:30:00 +0000</pubDate>
      </item>
    </channel>
  </rss>
XML

feed = RSS::Parser.parse(xml)
p feed.class
p feed.channel.title
p feed.channel.date.utc.strftime("%Y-%m-%d %H:%M:%S")
item = feed.items.first
p item.title
p item.date == item.pubDate
p item.date.utc.strftime("%Y-%m-%d %H:%M:%S")
p RSS::Rss::Channel::Item.instance_method(:date).original_name
