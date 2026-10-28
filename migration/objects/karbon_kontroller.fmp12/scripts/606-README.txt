# script 606: @README
# file: karbon_kontroller.fmp12
# folder: _OTTO_
# source: ScriptCatalog/Script[@id=606]
# steps: 3 (0 disabled)
# options: includeInMenu=True SiriShortcutVisible=False runFullAccess=False
# folder group: id=603 groupCollapsed=False includeInMenu=True

   1    #Put your post-migration scripts in the pending migration scripts folder
        DisableStepCollapsed state=False
        Restore state=False
   2    #If there is more than one migration script to be executed, create a parent that calls each one in a single transaction
        DisableStepCollapsed state=False
        Restore state=False
   3    #When an Otto migration is executed, move all scripts from pending to complete
        DisableStepCollapsed state=False
        Restore state=False
