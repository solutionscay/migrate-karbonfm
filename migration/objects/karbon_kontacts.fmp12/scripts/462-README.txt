# script 462: README
# file: karbon_kontacts.fmp12
# folder: Modules / BrowserNav
# source: ScriptCatalog/Script[@id=462]
# steps: 68 (0 disabled)
# options: includeInMenu=False SiriShortcutVisible=False runFullAccess=False
# folder group: id=456 groupCollapsed=True includeInMenu=False

   1    #
        DisableStepCollapsed state=False
        Restore state=False
   2    #================================================================================
        DisableStepCollapsed state=False
        Restore state=False
   3    #
        DisableStepCollapsed state=False
        Restore state=False
   4    #         B R O W S E R   N A V
        DisableStepCollapsed state=False
        Restore state=False
   5    #
        DisableStepCollapsed state=False
        Restore state=False
   6    #    Provides Browser like navigation history that is extremely easy to retrofit to an existing solution
        DisableStepCollapsed state=False
        Restore state=False
   7    #
        DisableStepCollapsed state=False
        Restore state=False
   8    #	Paul Jansen, paul.jansen@apjuk.com
        DisableStepCollapsed state=False
        Restore state=False
   9    #
        DisableStepCollapsed state=False
        Restore state=False
  10    #    Version 5.2 - 18 April 2013 Added NameSpace to all Global Variables
        DisableStepCollapsed state=False
        Restore state=False
  11    #    Version 5.3 - 19 April 2013 Added support for multiple windows
        DisableStepCollapsed state=False
        Restore state=False
  12    #    Version 5.4 - 19 April 2013 Added ability to NOT add a layout to the history by setting BrowserNav.SaveDisabled to True
        DisableStepCollapsed state=False
        Restore state=False
  13    #    Version 5.5 - 23 May 2013 Added ability have a common history accross many windows of same file
        DisableStepCollapsed state=False
        Restore state=False
  14    #    Version 6.0 - 1 August 2017 New version using JSON to store BrowserNav date
        DisableStepCollapsed state=False
        Restore state=False
  15    #    Version 6.1 - 20 September 2017 Navigte layouts using Layout Number rather than Layout name
        DisableStepCollapsed state=False
        Restore state=False
  16    #
        DisableStepCollapsed state=False
        Restore state=False
  17    #================================================================================
        DisableStepCollapsed state=False
        Restore state=False
  18    #
        DisableStepCollapsed state=False
        Restore state=False
  19    #INSTALLATION
        DisableStepCollapsed state=False
        Restore state=False
  20    #	Copy the single custom function into your file.
        DisableStepCollapsed state=False
        Restore state=False
  21    #	It is possible no avoid the use of the custom function, however I do not recommend this option as any changes
        	 to the 'Save layout to history' function will need to be applied to each layout.
        DisableStepCollapsed state=False
        Restore state=False
  22    #	Copy the BrowserNav script folder into your file
        DisableStepCollapsed state=False
        Restore state=False
  23    #	Copy and paste the back and forward buttons onto each layout you wish to include in the navigation history
        DisableStepCollapsed state=False
        Restore state=False
  24    #	Add a call to the script 'BrowserNav Settings' to your OnFirstWindowOpen trigger script.
        DisableStepCollapsed state=False
        Restore state=False
  25    #
        DisableStepCollapsed state=False
        Restore state=False
  26    #	The script 'BrowserName OnLayoutEnter' can be used instead of calling the 'BrowserNav.SaveNavHistory' 
        	custom function using conditional formatting on the back button
        DisableStepCollapsed state=False
        Restore state=False
  27    #	The 'BrowserNav.SaveNavHistory' custom function can be called in a Hide calculation instead of the 
        	conditional formatting on the back button
        DisableStepCollapsed state=False
        Restore state=False
  28    #
        DisableStepCollapsed state=False
        Restore state=False
  29    ### DEPENDANCIES ###
        DisableStepCollapsed state=False
        Restore state=False
  30    #	none
        DisableStepCollapsed state=False
        Restore state=False
  31    #
        DisableStepCollapsed state=False
        Restore state=False
  32    #### USAGE ###
        DisableStepCollapsed state=False
        Restore state=False
  33    #	Layouts with the navigation buttons are automatically included in the navigation history regardless
        	 of how you navigate to the layout.
        DisableStepCollapsed state=False
        Restore state=False
  34    #	Use the back and forward buttons to navigate through the layout history
        DisableStepCollapsed state=False
        Restore state=False
  35    #	If a script navigates to a layout not without the navigation buttons it will not be included in the history.
        DisableStepCollapsed state=False
        Restore state=False
  36    #	If the same script later returns to the original layout, this will not end up with a double entry 
        	in the navigation history.
        DisableStepCollapsed state=False
        Restore state=False
  37    #	The back and forward buttons comprise two elements, a button with a script and parameter and a text object 
        	with conditional formatting
        DisableStepCollapsed state=False
        Restore state=False
  38    #	The buttons on Layout 2 do not require the custom function, however I do not recomend this approach unless 
        	you do not have acces to Manage - Custom Functions
        DisableStepCollapsed state=False
        Restore state=False
  39    #	Set $$BrowserNav.SaveDisabled = True to temporarily disable the saving of layouts to the history 
        	(whilst running a scripted process for example)
        DisableStepCollapsed state=False
        Restore state=False
  40    #
        DisableStepCollapsed state=False
        Restore state=False
  41    #### CREDITS ###
        DisableStepCollapsed state=False
        Restore state=False
  42    #
        DisableStepCollapsed state=False
        Restore state=False
  43    #### LICENSE ###
        DisableStepCollapsed state=False
        Restore state=False
  44    #
        DisableStepCollapsed state=False
        Restore state=False
  45    #	The MIT License (MIT)
        DisableStepCollapsed state=False
        Restore state=False
  46    # 	Copyright (c) 2017 APJ Ltd, info@apjuk.com
        DisableStepCollapsed state=False
        Restore state=False
  47    #
        DisableStepCollapsed state=False
        Restore state=False
  48    # 	Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
        DisableStepCollapsed state=False
        Restore state=False
  49    #	associated documentation files (the "Software"), to deal in the Software without restriction, including
        DisableStepCollapsed state=False
        Restore state=False
  50    #	without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
        DisableStepCollapsed state=False
        Restore state=False
  51    #	copies of the Software, and to permit persons to whom the Software is furnished to do so, subject
        DisableStepCollapsed state=False
        Restore state=False
  52    #	to the following conditions:
        DisableStepCollapsed state=False
        Restore state=False
  53    #
        DisableStepCollapsed state=False
        Restore state=False
  54    # 	The above copyright notice and this permission notice shall be included in all copies or substantial
        DisableStepCollapsed state=False
        Restore state=False
  55    #	portions of the Software.
        DisableStepCollapsed state=False
        Restore state=False
  56    #
        DisableStepCollapsed state=False
        Restore state=False
  57    # 	THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
        DisableStepCollapsed state=False
        Restore state=False
  58    # 	INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
        DisableStepCollapsed state=False
        Restore state=False
  59    #	PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
        DisableStepCollapsed state=False
        Restore state=False
  60    #	HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
        DisableStepCollapsed state=False
        Restore state=False
  61    #	OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
        DisableStepCollapsed state=False
        Restore state=False
  62    #	SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
        DisableStepCollapsed state=False
        Restore state=False
  63    #
        DisableStepCollapsed state=False
        Restore state=False
  64    #
        DisableStepCollapsed state=False
        Restore state=False
  65    #
        DisableStepCollapsed state=False
        Restore state=False
  66    #
        DisableStepCollapsed state=False
        Restore state=False
  67    #
        DisableStepCollapsed state=False
        Restore state=False
  68    #
        DisableStepCollapsed state=False
        Restore state=False
