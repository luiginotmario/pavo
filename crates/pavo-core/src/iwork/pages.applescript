-- exports a Pages document: osascript - <in> <out> <pdf|docx|epub|txt>
on run argv
	set inFile to POSIX file (item 1 of argv)
	set outFile to POSIX file (item 2 of argv)
	set fmt to item 3 of argv
	set wasRunning to application "Pages" is running
	if not wasRunning then
		-- wake it in the background and wait until it can take commands
		tell application "Pages" to launch
		repeat 50 times
			try
				tell application "Pages" to count documents
				exit repeat
			on error
				delay 0.2
			end try
		end repeat
	end if
	tell application "Pages"
		set doc to open inFile
		if fmt is "pdf" then
			export doc to outFile as PDF
		else if fmt is "docx" then
			export doc to outFile as Microsoft Word
		else if fmt is "epub" then
			export doc to outFile as EPUB
		else
			export doc to outFile as unformatted text
		end if
		close doc saving no
		if not wasRunning then quit
	end tell
end run
