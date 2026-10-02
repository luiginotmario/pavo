-- exports a Keynote presentation: osascript - <in> <out> <pdf|pptx>
on run argv
	set inFile to POSIX file (item 1 of argv)
	set outFile to POSIX file (item 2 of argv)
	set fmt to item 3 of argv
	set wasRunning to application "Keynote" is running
	if not wasRunning then
		-- wake it in the background and wait until it can take commands
		tell application "Keynote" to launch
		repeat 50 times
			try
				tell application "Keynote" to count documents
				exit repeat
			on error
				delay 0.2
			end try
		end repeat
	end if
	tell application "Keynote"
		set doc to open inFile
		if fmt is "pdf" then
			export doc to outFile as PDF
		else
			export doc to outFile as Microsoft PowerPoint
		end if
		close doc saving no
		if not wasRunning then quit
	end tell
end run
