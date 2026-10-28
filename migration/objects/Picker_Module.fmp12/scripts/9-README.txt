# script 9: README
# file: Picker Module.fmp12
# folder: Modules / Contact Picker (Contacts Sample File)
# source: ScriptCatalog/Script[@id=9]
# steps: 58 (0 disabled)
# options: includeInMenu=False SiriShortcutVisible=False runFullAccess=False
# folder group: id=6 groupCollapsed=False includeInMenu=False

   1    #####################################################################################
        DisableStepCollapsed state=False
        Restore state=False
   2    #
        DisableStepCollapsed state=False
        Restore state=False
   3    #   Module Name: Contact Picker
        DisableStepCollapsed state=False
        Restore state=False
   4    #
        DisableStepCollapsed state=False
        Restore state=False
   5    #   Maintainer: Dave Graham, dave@geistinteractive.com
        DisableStepCollapsed state=False
        Restore state=False
   6    #
        DisableStepCollapsed state=False
        Restore state=False
   7    #   Description: 
        
        GOAL
        
        The Contact Picker Module's goal is to abstract the activity of searching for existing contacts, as well as adding new contacts. In most CRM solutions, it can be called from many different contexts. The first example (see Contacts Demo) is used when creating a new contact. In this scenario the picker is used to search the database first to prevent users from adding a duplicate contact. The second example (see Contacts Demo) is adding a related contact. Another potential use case is picking a contact to attach to an email, note, or other activity record. There are an infinite amount of use cases for this picker!
        
        METHODOLOGY
        
        Since almost every use case will have a workflow that will need to run after the contact is picked (or created) we need to have a method to allow the originating script to continue after the pick. We can't lock the user in a pause/resume loop because the magic which performs the searching will not run if a script is paused. For this reason we install a handler using the On Timer script in the file which is calling the picker. The handler's job is to "watch" the picker to see if a contact was picked, created, or canceled, after which it will continue along its path.
        
        RECOMMENDED USE
        
        This file should remain separate from your solution. Since it has minimal dependencies it is quite easy to integrate as a separate file, and because it's designed to work in a pop-up window there's almost no benefit to merging this into your primary contact database.
        DisableStepCollapsed state=False
        Restore state=False
   8    #
        DisableStepCollapsed state=False
        Restore state=False
   9    #####################################################################################
        DisableStepCollapsed state=False
        Restore state=False
  10    #
        DisableStepCollapsed state=False
        Restore state=False
  11    #
        DisableStepCollapsed state=False
        Restore state=False
  12    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  13    #
        DisableStepCollapsed state=False
        Restore state=False
  14    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  15    #Installation Steps
        DisableStepCollapsed state=False
        Restore state=False
  16    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  17    #
        DisableStepCollapsed state=False
        Restore state=False
  18    #Update external data source
        DisableStepCollapsed state=False
        Restore state=False
  19    #
        DisableStepCollapsed state=False
        Restore state=False
  20    #Point the Contacts table occurrence to your Contacts table.
        DisableStepCollapsed state=False
        Restore state=False
  21    #
        DisableStepCollapsed state=False
        Restore state=False
  22    #Update the fields on the Configuration Layout
        DisableStepCollapsed state=False
        Restore state=False
  23    #
        DisableStepCollapsed state=False
        Restore state=False
  24    #Update any private scripts (below) which have dependencies.
        DisableStepCollapsed state=False
        Restore state=False
  25    #
        DisableStepCollapsed state=False
        Restore state=False
  26    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  27    #
        DisableStepCollapsed state=False
        Restore state=False
  28    #
        DisableStepCollapsed state=False
        Restore state=False
  29    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  30    #Module Dependancies
        DisableStepCollapsed state=False
        Restore state=False
  31    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  32    #   Core
        DisableStepCollapsed state=False
        Restore state=False
  33    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  34    #
        DisableStepCollapsed state=False
        Restore state=False
  35    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  36    #Custom Functions
        DisableStepCollapsed state=False
        Restore state=False
  37    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  38    #
        DisableStepCollapsed state=False
        Restore state=False
  39    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  40    #
        DisableStepCollapsed state=False
        Restore state=False
  41    #
        DisableStepCollapsed state=False
        Restore state=False
  42    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  43    #Tables
        DisableStepCollapsed state=False
        Restore state=False
  44    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  45    #
        DisableStepCollapsed state=False
        Restore state=False
  46    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  47    #
        DisableStepCollapsed state=False
        Restore state=False
  48    #
        DisableStepCollapsed state=False
        Restore state=False
  49    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  50    #Layouts
        DisableStepCollapsed state=False
        Restore state=False
  51    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  52    #
        DisableStepCollapsed state=False
        Restore state=False
  53    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  54    #
        DisableStepCollapsed state=False
        Restore state=False
  55    #
        DisableStepCollapsed state=False
        Restore state=False
  56    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
  57    #Script Folder
        DisableStepCollapsed state=False
        Restore state=False
  58    #----------------------------------------
        DisableStepCollapsed state=False
        Restore state=False
